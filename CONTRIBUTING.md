# Contributing

## Issues

Report an Issue if:

- You find a bug.
- You have a suggestion to make.

## Contributing Code

### Commit rules involve

- A individual commit should be able to compile on its own.
- The code should be formatted & tested before committing via `cargo fmt` & `cargo test`.
- The code should follow the Minimum Rust Version (MRV) of the project.
- Commit should not be long enough to make reviewing harder.

### Commit messages

Follow a generalized pattern for committing work

- `feat`: A new marginal/small feature is added to the project.
- `fix`: A bug is fixed.
- `core`: A core feature is added to the project.
- `feat!`: A new feature is added with breaking changes.
- `chore`: Making changes that is necessary but doesn't boost any part of the project (Code cleanup, removing print logs, updating crates etc).
