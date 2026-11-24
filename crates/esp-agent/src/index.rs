//! A brute-force in-memory vector index with a payload per entry.

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// A stored vector with its payload, found by [`Index::search`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit<'a, T> {
    /// Cosine similarity between the query and the stored vector.
    pub score: f32,
    /// Insertion position of the entry, starting at zero.
    pub index: usize,
    /// The payload stored with the vector.
    pub payload: &'a T,
}

/// The worst candidate sorts last, placing it at the root of the max-heap.
struct Candidate {
    score: f32,
    index: usize,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .total_cmp(&self.score)
            .then(self.index.cmp(&other.index))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Candidate {}

/// Stores vectors with a payload each and returns the ones closest to a query.
///
/// Every vector has [`dim`](Self::dim) components. The vectors live in one contiguous
/// buffer of `len * dim` floats, next to a list of payloads, and a search scores every
/// entry by cosine similarity, so it works for the few hundred entries that fit in the
/// memory of a chip. Scores are cosine similarities, not raw dot products, so they stay
/// correct for vectors that are not unit length.
///
/// Hits come back by descending score. Entries with equal scores keep their insertion
/// order, so a search is deterministic and a `k` that cuts through a tie keeps the
/// earliest entries.
#[derive(Debug, Clone)]
pub struct Index<T> {
    dim: usize,
    /// `len * dim` floats; row `i` belongs to `payloads[i]`.
    vectors: Vec<f32>,
    payloads: Vec<T>,
}

impl<T> Index<T> {
    /// Creates an empty index of vectors with `dim` components.
    #[must_use]
    pub const fn new(dim: usize) -> Self {
        Self {
            dim,
            vectors: Vec::new(),
            payloads: Vec::new(),
        }
    }

    /// Creates an empty index that can hold `capacity` entries without reallocating.
    #[must_use]
    pub fn with_capacity(dim: usize, capacity: usize) -> Self {
        Self {
            dim,
            vectors: Vec::with_capacity(dim.saturating_mul(capacity)),
            payloads: Vec::with_capacity(capacity),
        }
    }

    /// Returns the number of components in every vector.
    #[must_use]
    pub const fn dim(&self) -> usize {
        self.dim
    }

    /// Returns the number of entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.payloads.len()
    }

    /// Returns `true` when the index holds no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.payloads.is_empty()
    }

    /// Appends `vector` with its `payload`.
    ///
    /// # Panics
    ///
    /// Panics if `vector.len()` is not equal to [`dim`](Self::dim).
    pub fn insert(&mut self, vector: &[f32], payload: T) {
        assert_eq!(vector.len(), self.dim, "vector must have `dim` components");
        self.vectors.extend_from_slice(vector);
        self.payloads.push(payload);
    }

    /// Returns the `k` entries closest to `query`, best first.
    ///
    /// Fewer than `k` hits come back when the index holds fewer entries, and none when
    /// `k` is zero. A query of all zeros scores every entry `0.0`, so the hits are the
    /// first entries in insertion order.
    ///
    /// # Panics
    ///
    /// Panics if `query.len()` is not equal to [`dim`](Self::dim).
    #[must_use]
    pub fn search(&self, query: &[f32], k: usize) -> Vec<Hit<'_, T>> {
        self.search_with(query, k, |hit| hit)
    }

    pub(crate) fn search_with<'a, H>(
        &'a self,
        query: &[f32],
        k: usize,
        hit: impl Fn(Hit<'a, T>) -> H,
    ) -> Vec<H> {
        assert_eq!(query.len(), self.dim, "query must have `dim` components");
        let limit = k.min(self.len());
        if limit == 0 {
            return Vec::new();
        }

        let query_squared_norm = dot(query, query);
        let query_norm = libm::sqrtf(query_squared_norm);
        let mut wide_query_norm = None;
        let mut best = BinaryHeap::with_capacity(limit);
        for index in 0..self.len() {
            let row = self.row(index);
            let row_squared_norm = dot(row, row);
            let score = if query_squared_norm.is_normal() && row_squared_norm.is_normal() {
                normalized_dot(query, row, query_norm * libm::sqrtf(row_squared_norm))
            } else {
                // Squared norms can underflow or overflow in f32.
                let query_norm =
                    *wide_query_norm.get_or_insert_with(|| libm::sqrt(wide_dot(query, query)));
                let denominator = query_norm * libm::sqrt(wide_dot(row, row));
                if denominator == 0.0 {
                    0.0
                } else {
                    (wide_dot(query, row) / denominator) as f32
                }
            };
            // Canonicalize signed zero so equally close candidates tie.
            let score = score + 0.0;
            let candidate = Candidate { score, index };
            if best.len() < limit {
                best.push(candidate);
            } else if let Some(mut worst) = best.peek_mut()
                && candidate < *worst
            {
                *worst = candidate;
            }
        }

        let mut hits = Vec::with_capacity(limit);
        hits.extend(best.into_sorted_vec().into_iter().map(|candidate| {
            hit(Hit {
                score: candidate.score,
                index: candidate.index,
                payload: &self.payloads[candidate.index],
            })
        }));
        hits
    }

    /// Returns the vector of entry `index`.
    fn row(&self, index: usize) -> &[f32] {
        &self.vectors[index * self.dim..(index + 1) * self.dim]
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalized_dot(a: &[f32], b: &[f32], denominator: f32) -> f32 {
    // Keep ordinary terms at firmware precision even when another product underflows.
    let mut sum = 0.0;
    let mut tiny_sum = 0.0;
    for (&x, &y) in a.iter().zip(b) {
        let product = x * y;
        if product.abs() < f32::MIN_POSITIVE && x != 0.0 && y != 0.0 {
            tiny_sum += f64::from(x) * f64::from(y);
        } else {
            sum += product;
        }
    }
    if tiny_sum == 0.0 {
        sum / denominator
    } else {
        ((f64::from(sum) + tiny_sum) / f64::from(denominator)) as f32
    }
}

fn wide_dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x) * f64::from(y))
        .sum()
}

impl<T, V> Extend<(V, T)> for Index<T>
where
    V: AsRef<[f32]>,
{
    /// Inserts every `(vector, payload)` pair.
    ///
    /// # Panics
    ///
    /// Panics if a vector does not have [`dim`](Index::dim) components.
    fn extend<I: IntoIterator<Item = (V, T)>>(&mut self, iter: I) {
        for (vector, payload) in iter {
            self.insert(vector.as_ref(), payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::Index;

    fn three_entries() -> Index<char> {
        let mut index = Index::new(2);
        index.insert(&[1.0, 0.0], 'a');
        index.insert(&[0.0, 1.0], 'b');
        index.insert(&[1.0, 1.0], 'c');
        index
    }

    #[test]
    fn search_orders_hits_by_descending_score() {
        let mut index = three_entries();
        index.insert(&[-1.0, 0.0], 'd');
        index.insert(&[-1.0, 1.0], 'e');
        let hits = index.search(&[1.0, 0.0], 5);

        let payloads: Vec<char> = hits.iter().map(|hit| *hit.payload).collect();
        let indices: Vec<usize> = hits.iter().map(|hit| hit.index).collect();
        assert_eq!(payloads, ['a', 'c', 'b', 'e', 'd']);
        assert_eq!(indices, [0, 2, 1, 4, 3]);
        let diagonal = core::f32::consts::FRAC_1_SQRT_2;
        let expected = [1.0, diagonal, 0.0, -diagonal, -1.0];
        for (hit, expected) in hits.iter().zip(expected) {
            let score = hit.score;
            assert!((score - expected).abs() < 1e-6, "score was {score}");
        }
    }

    #[test]
    fn zero_query_returns_hits_in_insertion_order_for_non_positive_vectors() {
        let mut index = Index::new(2);
        index.insert(&[-0.6, -0.8], 'a');
        index.insert(&[0.6, 0.8], 'b');
        index.insert(&[0.0, 0.0], 'c');

        let indices: Vec<usize> = index
            .search(&[0.0, 0.0], 3)
            .iter()
            .map(|hit| hit.index)
            .collect();
        assert_eq!(indices, [0, 1, 2]);
    }

    #[test]
    fn search_returns_every_entry_when_k_exceeds_len() {
        assert_eq!(three_entries().search(&[1.0, 0.0], 10).len(), 3);
    }

    #[test]
    fn search_limits_result_capacity() {
        let index = three_entries();
        assert_eq!(index.search(&[1.0, 0.0], 0).capacity(), 0);
        let hits = index.search(&[0.0, 1.0], 1);
        assert_eq!(hits.capacity(), 1);
        assert_eq!(hits[0].index, 1);
    }

    #[test]
    fn empty_index_returns_nothing() {
        let index = Index::<char>::new(2);
        assert_eq!(index.len(), 0);
        assert!(index.is_empty());
        assert_eq!(index.dim(), 2);
        assert!(index.search(&[1.0, 0.0], 3).is_empty());
    }

    #[test]
    fn equal_scores_keep_insertion_order() {
        let mut index = Index::new(2);
        index.insert(&[1.0, 0.0], 'x');
        index.insert(&[1.0, 0.0], 'y');
        index.insert(&[1.0, 0.0], 'z');

        let indices: Vec<usize> = index
            .search(&[1.0, 0.0], 2)
            .iter()
            .map(|hit| hit.index)
            .collect();
        assert_eq!(indices, [0, 1]);
    }

    #[test]
    fn scores_are_cosine_similarity_for_unnormalized_vectors() {
        for scale in [1e-7, 1e20, f32::from_bits(1), f32::MAX] {
            let mut index = Index::new(2);
            index.insert(&[1.0, 1.0], 'a');
            index.insert(&[scale, 0.0], 'b');
            let hits = index.search(&[scale, 0.0], 1);
            assert_eq!(hits[0].index, 1, "scale was {scale}");
            assert_eq!(hits[0].score, 1.0, "scale was {scale}");
        }
    }

    #[test]
    fn cancellation_at_firmware_precision_keeps_insertion_order() {
        let mut index = Index::new(3);
        index.insert(&[1.0, 0.0, -1.0], 'a');
        index.insert(&[1.0, f32::EPSILON / 4.0, -1.0], 'b');

        let hits = index.search(&[1.0; 3], 1);
        assert_eq!(hits[0].index, 0);
        assert_eq!(hits[0].score, 0.0);
    }

    #[test]
    fn cancelling_tiny_products_preserve_a_firmware_precision_tie() {
        let tiny = 1e-30;
        let epsilon = f32::EPSILON / 4.0;
        let query = [1.0, 1.0, 1.0, tiny, tiny, 0.0, 0.0];
        let mut index = Index::new(query.len());
        index.insert(&[1.0, epsilon, -1.0, 0.0, 0.0, tiny, -tiny], 'a');
        index.insert(&[1.0, epsilon, -1.0, tiny, -tiny, 0.0, 0.0], 'b');

        let hits = index.search(&query, 1);
        assert_eq!(hits[0].index, 0);
        let hits = index.search(&query, usize::MAX);
        assert_eq!(hits[0].score, hits[1].score);
    }

    #[test]
    fn search_ranks_tiny_cross_products_above_orthogonal_vectors() {
        let scale = libm::ldexpf(1.0, -63);
        let overlap = libm::ldexpf(1.0, -88);
        let mut index = Index::new(2);
        index.insert(&[0.0, scale], 'a');
        index.insert(&[overlap, scale], 'b');

        let hits = index.search(&[scale, 0.0], 1);
        assert_eq!(hits[0].index, 1);
        assert_eq!(hits[0].score, overlap / scale);
    }

    #[test]
    #[should_panic(expected = "vector must have `dim` components")]
    fn insert_rejects_vectors_of_another_dimension() {
        Index::new(2).insert(&[1.0, 0.0, 0.0], 'a');
    }

    #[test]
    #[should_panic(expected = "query must have `dim` components")]
    fn search_rejects_queries_of_another_dimension() {
        let _ = Index::<char>::new(2).search(&[1.0], 1);
    }

    #[test]
    fn extend_inserts_every_pair() {
        let mut index = Index::new(2);
        index.extend([(vec![1.0, 0.0], 'x'), (vec![0.0, 1.0], 'y')]);

        assert_eq!(index.len(), 2);
        assert_eq!(index.search(&[0.0, 1.0], 1)[0].payload, &'y');
    }
}
