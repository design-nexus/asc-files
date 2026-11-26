# Files, the file manager of AscendantOS. Built from a source tarball and a tarball of the vendored
# Rust crates (scripts/rpm.sh makes both), offline.

Name:           asc-files
Version:        0.1.0
Release:        1%{?dist}
Summary:        The file manager of AscendantOS
# Files itself is MIT; the vendored crates are MIT, Apache-2.0, BSD, Zlib, Unicode and ISC
# (THIRD_PARTY_LICENSES.md); MathJax (bundled for formulas) is Apache-2.0.
License:        MIT AND Apache-2.0 AND BSD-3-Clause AND Zlib AND Unicode-3.0 AND ISC
URL:            https://github.com/design-nexus/asc-files
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz

BuildRequires:  cargo >= 1.100
BuildRequires:  rust >= 1.100
BuildRequires:  cmake
BuildRequires:  ninja-build
BuildRequires:  gcc-c++
BuildRequires:  pkgconfig(gio-2.0)
BuildRequires:  qt6-qtbase-devel
BuildRequires:  qt6-qtdeclarative-devel
BuildRequires:  qt6-qtmultimedia-devel
BuildRequires:  qt6-qttools-devel
BuildRequires:  qt6-linguist
BuildRequires:  kf6-syntax-highlighting-devel
BuildRequires:  desktop-file-utils
BuildRequires:  appstream

# Previews are read in a sandbox; PDFs and video frames by these.
Requires:       bubblewrap
Requires:       qt6-qtsvg
Requires:       hicolor-icon-theme
Recommends:     poppler-utils
Recommends:     ffmpeg-free
Recommends:     gvfs
Recommends:     udisks2

%description
The file manager of AscendantOS: list, icon, column, compact, gallery and dual-pane views,
previews of documents, pictures, media, archives and 3D models read in a sandbox, search,
drives and network locations, undo for every operation, and a keyboard-first 10xer mode.

%package        rar
Summary:        RAR archives for Files
# RarLab's UnRAR licence: free to use and redistribute, not to make a RAR compressor.
License:        LicenseRef-UnRAR
Requires:       %{name}%{?_isa} = %{version}-%{release}

%description    rar
Lets Files list and extract RAR archives, with RarLab's UnRAR code, separately licensed; it runs
in Files' sandbox.

%prep
%autosetup -a 1
mkdir -p .cargo
cat > .cargo/config.toml <<'CONFIG'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CONFIG

%build
export CARGO_HOME="$PWD/.cargo"
cargo build --release --frozen --offline \
    -p asc-files-backend -p asc-files-docs -p asc-files-rar
%cmake -S app -G Ninja \
    -DASC_FILES_BUILD_TESTS=OFF \
    -DASC_FILES_HELPERS_DIR="$PWD/target/release" \
    -DASC_FILES_COMMIT=%{release}
%cmake_build

%install
%cmake_install
DESTDIR=%{buildroot} %__cmake --install %{__cmake_builddir} --component rar

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/asc-files.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/io.github.design_nexus.ascendant.Files.metainfo.xml

%files
%license LICENSE THIRD_PARTY_LICENSES.md
%doc README.md
%{_bindir}/asc-files
%dir %{_libexecdir}/asc-files
%{_libexecdir}/asc-files/asc-files-backend
%{_libexecdir}/asc-files/asc-files-docs
%{_libexecdir}/asc-files/asc-files-render
%{_datadir}/applications/asc-files.desktop
%{_datadir}/icons/hicolor/scalable/apps/asc-files.svg
%{_metainfodir}/io.github.design_nexus.ascendant.Files.metainfo.xml
%{_datadir}/dbus-1/services/org.freedesktop.impl.portal.desktop.asc_files.service
%{_datadir}/xdg-desktop-portal/portals/asc_files.portal

%files rar
%license crates/asc-files-rar/LICENSE
%{_libexecdir}/asc-files/asc-files-rar

%changelog
* Thu Nov 26 2026 Design Nexus <noreply@github.com> - 0.1.0-1
- First package.
