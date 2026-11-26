// SPDX-License-Identifier: MIT
#include "ClipboardMarks.h"

ClipboardMarks &ClipboardMarks::shared() {
    static ClipboardMarks marks;
    return marks;
}

void ClipboardMarks::set(const QList<QByteArray> &paths, bool cut) {
    m_paths = QSet<QByteArray>(paths.cbegin(), paths.cend());
    m_cut = cut;
    emit changed();
}

void ClipboardMarks::clear() {
    if (m_paths.isEmpty()) {
        return;
    }
    m_paths.clear();
    emit changed();
}

ClipboardMarks::Mark ClipboardMarks::markOf(const QByteArray &path) const {
    return !m_paths.contains(path) ? None : m_cut ? Cut : Copied;
}
