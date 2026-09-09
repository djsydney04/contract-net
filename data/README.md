# Runtime learning

The Rust application stores bounded execution and settlement observations in
`runtime/`, separated by manager URL, room, and team name. This directory is
ignored by Git. Credentials and task results are not stored in those files.
Use `--state PATH` to choose a file or `--no-state` for an in-memory session.
