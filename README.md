# Rust OpenAPI 3.1 Type Generator

<!-- prettier-ignore-start -->
[![crates.io](https://img.shields.io/crates/v/oas3-gen?label=latest)](https://crates.io/crates/oas3-gen)
[![Documentation](https://docs.rs/oas3-gen/badge.svg?version=0.1.0)](https://docs.rs/oas3-gen/0.1.0)
[![dependency status](https://deps.rs/crate/oas3-gen/0.1.0/status.svg)](https://deps.rs/crate/oas3-gen/0.1.0)
![MIT licensed](https://img.shields.io/crates/l/oas3-gen.svg)
<br />
![Version](https://img.shields.io/crates/msrv/oas3-gen.svg)
[![Download](https://img.shields.io/crates/d/oas3-gen.svg)](https://crates.io/crates/oas3-gen)
<!-- prettier-ignore-end -->

## Example

```md
A rust type generator for OpenAPI v3.1.x specification.

Usage: oas3-gen [OPTIONS] --input <FILE> --output <FILE>

Options:
  -i, --input <FILE>   Path to the OpenAPI JSON specification file
  -o, --output <FILE>  Path where the generated Rust code will be written
  -v, --verbose        Enable verbose output with detailed progress information
  -q, --quiet          Suppress non-essential output (errors only)
  -h, --help           Print help
  -V, --version        Print version
```

## License

Copyright (c) 2025 Individual contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual-licensed as above, without any additional terms or
conditions.

See [CONTRIBUTING](CONTRIBUTING.md) for more details.

See [OpenAPI v3.1.x]: <https://spec.openapis.org/oas/v3.1.1>
