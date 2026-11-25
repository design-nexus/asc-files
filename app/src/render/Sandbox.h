// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QImage>
#include <QObject>
#include <QString>
#include <QStringList>
#include <functional>

/// Runs a decoder on one file where it can do no harm: inside bubblewrap with no network, no
/// home folder and nothing writable, the file alone bound in read-only, and limits on memory
/// (1 GiB), CPU time (10 s) and wall time (15 s). Where bubblewrap is missing it still runs
/// out of the app's process, under the same limits, and `sandboxed()` says so.
namespace Sandbox {

/// Whether bubblewrap was found.
bool sandboxed();

/// A program on PATH, resolved to its real file (a decoder Files runs), or empty.
QString program(const QString &name);

/// The helper that decodes images, `asc-files-render`, beside the app or on PATH.
QString renderer();

/// The helper that reads documents, `asc-files-docs`: beside the app, else as
/// `ASC_FILES_DOCS` names it, else on PATH.
QString documentReader();

/// The RAR reader, separately licensed: beside the app, then `ASC_FILES_RAR`, then the PATH.
QString rarReader();

/// Runs `program` with `args` on `file` (inside the sandbox, `file` is `/input`; write `{}` in
/// `args` where it goes). `done` gets what it wrote to stdout, or the error.
void run(const QString &program, const QStringList &args, const QByteArray &file, QObject *context,
         std::function<void(const QByteArray &out, const QString &error)> done);

/// Decodes the image `file` (raw bytes) no larger than `size` per side, in the sandbox.
void decodeImage(const QByteArray &file, int size, QObject *context,
                 std::function<void(const QImage &image, const QString &error)> done);

/// Decodes the image `file` synchronously (for worker threads), in the sandbox.
QImage decodeImageNow(const QByteArray &file, int size);

/// `run`, synchronously (for worker threads): what it wrote, or nothing when it failed.
QByteArray runNow(const QString &program, const QStringList &args, const QByteArray &file);

/// A frame of the video `file`, no larger than `size` per side, synchronously, in the sandbox.
QImage videoFrameNow(const QByteArray &file, int size);

/// The PDF `file`'s first page, no larger than `size` per side, synchronously, in the sandbox.
QImage pdfPageNow(const QByteArray &file, int size);

/// The cover a comic, e-book or model carries (`format`: cbz, epub, 3mf or fcstd), read by the
/// document reader, at most `size` a side; null when there is none. Blocking, as `runNow`.
QImage coverNow(const QByteArray &file, const QString &format, int size);

/// A RAR comic's (CBR's) first page, read by the RAR reader and decoded, both in the sandbox; null
/// when there is none or no RAR reader. Blocking, as `runNow`.
QImage rarCoverNow(const QByteArray &file, int size);

} // namespace Sandbox
