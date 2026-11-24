// SPDX-License-Identifier: MIT
#pragma once

#include <QDir>
#include <QTemporaryDir>
#include <QTest>

/// What the app's tests share: the backend binary, and a guard that they run in the private home
/// scripts/app.sh makes, never in a real one.
namespace TestSupport {

inline QString backend() {
    const QString program = qEnvironmentVariable("ASC_FILES_BACKEND");
    if (program.isEmpty()) {
        qFatal("ASC_FILES_BACKEND is not set: run the tests through scripts/app.sh");
    }
    return program;
}

/// Fails unless HOME is the test home, so a Trash or undo history the tests touch is never real.
inline void requirePrivateHome() {
    const QString home = QDir::homePath();
    if (!home.contains(QLatin1String("asc-files-test-home"))) {
        qFatal("refusing to run outside the private test home (HOME=%s): use scripts/app.sh", qPrintable(home));
    }
}

} // namespace TestSupport
