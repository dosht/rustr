# Preface {.unnumbered}

This book grew out of one question: *how do I build one product that runs as a command line tool, a terminal UI, a web app, an Android app and an iOS app, without writing it five times?*

The answer it develops is a pattern called **Core and Shells**. All the rules and all the screen behavior live once, in a Rust core. Each platform is a thin shell that draws what the core decides and forwards what the user does. Everything in the book is tied to a real, running sample called **taskr**. It's a small task list, but it has a real domain, a real cross-cutting concern (telemetry) and all five shells.

## Who this is for {-}

You know how to build software and have shipped at least one app. You may know MVVM from React, Android or iOS, and you have heard of clean architecture and domain-driven design. You don't need to know Rust, Svelte, Kotlin or Swift. Each gets a primer at the point where the book needs it, using code from taskr instead of toy examples.

## How to read it {-}

- **Part one** (chapters 1 to 3) is the idea: why one core, what clean architecture actually asks of you, and the Core and Shells pattern itself.
- **Part two** (chapters 4 to 7) is the core in Rust: the language essentials you need, the domain and use cases, the shared ViewModel, and how to test all of it first.
- **Part three** (chapters 8 to 10) is the shells: CLI and TUI, web through WebAssembly and Svelte, and mobile through UniFFI with Kotlin and Swift.
- **Part four** (chapters 11 and 12) grows the system: databases, identity and RBAC, desktop apps, servers on Kubernetes, realtime, messaging, analytics, websites and a family of apps that share one platform.
- The **reference** at the end has a glossary, the decision log, checklists and every command.

Every code listing marked with a source path under it is copied from the repository, and the build fails if the code moves on and the book doesn't. Listings without a path are sketches of things the sample doesn't do yet.

## A living book {-}

This book grows with the sample. When taskr gains a server, a database or a second bounded context, the chapters that describe them change, and the edition line on the title page changes with them. The reference chapter ends with a changelog so you can see what's new since the copy you last read.

To rebuild it yourself:

```sh
./scripts/build-book.sh          # writes book/build/core-and-shells.epub and .pdf
./scripts/build-book.sh --check  # only checks that code listings still match the source
```
