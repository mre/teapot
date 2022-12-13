# 🫖 teapot

A super simple dummy server for testing HTTP clients.
Zero dependencies.

## Installation

```bash
cargo install --git https://github.com/mre/teapot.git
```

## Usage

```bash
teapot
```

This will always respond with a `200`; no matter what.  
You can configure the port with the `PORT` environment variable.

```
PORT=8080 teapot
```

## Credits

Based on [mockserver](https://github.com/takakawa/mockserver) by
[takakawa](https://github.com/takakawa).
