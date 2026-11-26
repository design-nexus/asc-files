// SPDX-License-Identifier: MIT
#pragma once

#include <QDir>
#include <QFileInfo>
#include <QString>

namespace Helpers {

/// `name` where an installed Files keeps its helpers (`/usr/libexec/asc-files`), when it is there.
inline QString installed(const QString &name) {
#ifdef ASC_FILES_LIBEXEC_DIR
    const QString path = QDir(QStringLiteral(ASC_FILES_LIBEXEC_DIR)).filePath(name);
    if (QFileInfo(path).isExecutable()) {
        return QFileInfo(path).canonicalFilePath();
    }
#else
    Q_UNUSED(name)
#endif
    return {};
}

} // namespace Helpers
