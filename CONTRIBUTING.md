# Contributing

We are so happy you are interested in contributing to this project! We welcome contributions in the form of pull requests, bug reports, and feature requests.

## How to Contribute

- **Reporting Bugs:** If you find a bug, please open an issue on our GitHub repository. Please include as much detail as possible, including the steps to reproduce the bug.
- **Suggesting Enhancements:** If you have an idea for a new feature or an improvement to an existing one, please open an issue to discuss it. This allows us to coordinate our efforts and avoid duplicated work.
- **Pull Requests:** Pull requests are welcome. Please ensure that your code follows the project's style guidelines and that all tests pass.

## Pull Request Process

1. Ensure any install or build dependencies are removed before the end of the layer when doing a build.
2. Update the README.md with details of changes to the interface, this includes new environment variables, exposed ports, useful file locations and container parameters.
3. Increase the version numbers in any examples and the README.md to the new version that this Pull Request would represent. The versioning scheme we use is [SemVer](http.semver.org/).
4. You may merge the Pull Request in once you have the sign-off of two other developers, or if you do not have permission to do that, you may request the second reviewer to merge it for you.

## Styleguides

### Rust Style

Please follow the standard Rust formatting guidelines by using `rustfmt`. The formatting rules are defined in `rustfmt.toml` and are enforced as part of the pull request process.

## Dependency Management

We use `cargo deny` to manage our crate dependencies and avoid dependency bloat. Our configuration is in `deny.toml`. You can learn more about `cargo-deny` in the [cargo-deny documentation](https://embarkstudios.github.io/cargo-deny/index.html).

## License

All contributions to this project will be licensed under the MIT license.
