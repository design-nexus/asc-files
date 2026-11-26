// SPDX-License-Identifier: MIT
#pragma once

#include <QObject>
#include <QSet>

/// What Files put on the clipboard, for the items' marks: a copy or scissors badge on each, in
/// every view and window, until another program takes the clipboard or a cut is pasted.
class ClipboardMarks : public QObject {
    Q_OBJECT
public:
    enum Mark { None = 0, Copied = 1, Cut = 2 };

    static ClipboardMarks &shared();

    void set(const QList<QByteArray> &paths, bool cut);
    void clear();
    Mark markOf(const QByteArray &path) const;
    bool isEmpty() const { return m_paths.isEmpty(); }

signals:
    void changed();

private:
    QSet<QByteArray> m_paths;
    bool m_cut = false;
};
