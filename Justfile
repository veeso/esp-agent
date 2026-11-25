import "./just/bench.just"
import "./just/build.just"
import "./just/changelog.just"
import "./just/code_check.just"
import "./just/docs.just"
import "./just/publish.just"

# List every available command.
default:
    @just --list
