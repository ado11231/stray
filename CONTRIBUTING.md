# Contributing

* Short rules for changing Stray's code.
* Run the checks at the bottom before every commit.

## Errors

* Do not use `unwrap` or `expect`. Clippy blocks them in the program.
* Use `?` and add context, such as `.context("could not open PTY")`.
* When something being missing is normal, use `match` or `if let` on the `Option`.

## Clones

* Borrow with `&` when the caller still needs the value.
* Move the value when the caller is done with it.
* Clone only when two owners really need it, and say why in a comment.

## Names

* Put repeated or unclear values in a named `const`, not inline.
* Terminal codes go in `src/escape.rs`. Environment variable names go in `src/env.rs`.

## Comments

* Every file starts with a `//!` line that says what it is for.
* Comments explain why, in plain words.
* Every `unsafe` block has a `// SAFETY:` comment.
* No em dashes.

## Tests

* Tests return `Result` and use `?` instead of `unwrap`.
* Keep tests next to the code they check. Tests that run the whole program go in `tests/`.

## Before You Commit

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
```

* Keep commits small: one change per commit.
* Start the message with `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, or `chore:`.
