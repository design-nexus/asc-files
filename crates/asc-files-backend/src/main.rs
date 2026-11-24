// SPDX-License-Identifier: MIT
//! The Files backend process: serves the protocol on stdin and stdout.

fn main() {
    asc_files_backend::serve(std::io::stdin().lock(), std::io::stdout());
}
