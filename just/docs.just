# Build the documentation website into docs/book.
[group('docs')]
book args="":
  mdbook build docs {{ args }}

# Serve the documentation website locally and rebuild it on changes.
[group('docs')]
book_serve args="":
  mdbook serve docs --open {{ args }}
