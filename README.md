# Stray

* A small cat that lives in your terminal.
* It starts afraid of you. The more hours your terminal is open, the more it trusts you.
* Early work. Only an image test runs so far.

## Try The Image Test

* Needs Rust 1.89 or newer, and kitty or the VS Code terminal.
* In VS Code, turn on `terminal.integrated.enableImages` first.

```sh
cargo run --release --bin stray-image-test -- --resend
```

* A placeholder cat runs along the bottom of the window and back.
* Press q or Ctrl C to stop early.

## Docs

* [Design](docs/DESIGN.md): what Stray is and how the cat behaves.
* [Architecture](docs/ARCHITECTURE.md): how it is built.
* [Roadmap](docs/ROADMAP.md): what is done and what comes next.
