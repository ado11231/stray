# Stray

* A small cat that lives in your terminal.
* Right now Stray wraps your shell and draws a sitting cat, its bowl, and a cat tree in two rows at the bottom. The cat does not move yet.
* Works with bash and zsh, in kitty and the VS Code terminal.

## Run It

* Needs Rust 1.89 or newer.
* In VS Code, turn on `terminal.integrated.enableImages` first.

```sh
cargo run --release --bin stray -- start --login
```

* This opens your shell inside Stray. Type `exit` to leave.
* If the terminal cannot show images, Stray starts your shell normally.

## Install

```sh
cargo install --path .
stray install
```

* `stray install` adds one line to your `.zshrc` or bash profile, so new windows open with Stray.
* `stray uninstall` removes that line.

## Docs

* [Design](docs/DESIGN.md): what Stray is and how the cat behaves.
* [Architecture](docs/ARCHITECTURE.md): how it is built.
* [Roadmap](docs/ROADMAP.md): what is done and what comes next.
* [Contributing](CONTRIBUTING.md): code rules and checks to run before a commit.
