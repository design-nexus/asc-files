#!/usr/bin/env bash
# Builds the RPMs: a source tarball of HEAD, a tarball of the vendored crates, then rpmbuild.
# Run on Fedora (CI runs it in fedora:43); the packages land in dist/.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -n 's/^Version: *//p' packaging/rpm/asc-files.spec)
name=asc-files-$version
dist=$PWD/dist
rm -rf "$dist"
mkdir -p "$dist/sources"
git archive --format=tar.gz --prefix="$name/" -o "$dist/sources/$name.tar.gz" HEAD
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git archive --format=tar HEAD | tar -x -C "$work"
(cd "$work" && cargo vendor --locked --versioned-dirs vendor >/dev/null)
tar -C "$work" --transform "s|^|$name/|" -czf "$dist/sources/$name-vendor.tar.gz" vendor
rpmbuild -ba packaging/rpm/asc-files.spec \
    --define "_sourcedir $dist/sources" \
    --define "_topdir $dist/rpmbuild"
find "$dist/rpmbuild/RPMS" "$dist/rpmbuild/SRPMS" -name '*.rpm' -exec mv {} "$dist" \;
ls "$dist"/*.rpm
