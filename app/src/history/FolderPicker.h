// SPDX-License-Identifier: MIT
#pragma once

#include <QHash>
#include <QObject>
#include <QStringList>

#include <atomic>
#include <memory>

#include "Visits.h"

/// 10xer's folder picker (g Space, M, C, ; E): the folders below a folder whose paths match what
/// is typed (search's terms), found off the UI thread. Text that starts as a path (`/`, `~`,
/// `./`, `../`, `..`) moves where it searches: through its last `/` is the folder, the rest the
/// query.
class FolderPicker : public QObject {
    Q_OBJECT
    Q_PROPERTY(QStringList folders READ folders NOTIFY changed)
    Q_PROPERTY(bool searching READ searching NOTIFY changed)
    /// Why nothing can be listed for the text ("Type a full path here"), or empty.
    Q_PROPERTY(QString message READ message NOTIFY changed)
    /// Folders opened often and lately rank first.
    Q_PROPERTY(Visits *visits MEMBER m_visits)
public:
    static constexpr int Listed = 100;
    /// Folders looked through at most, so a search from `/` ends.
    static constexpr int Walked = 50000;

    explicit FolderPicker(Visits *visits = nullptr, QObject *parent = nullptr);
    ~FolderPicker() override;

    struct Where {
        QString base;
        QString query;
        QString error;
        /// The text ends in `/`: the folder itself comes first.
        bool itself = false;
    };
    /// Where `text` searches, typed in `open` (empty when the folder shown isn't local).
    static Where where(const QString &text, const QString &open);
    /// The folders below `base` matching `query`, best first (no more than `Listed`).
    static QStringList find(const Where &where, bool hidden, const QHash<QString, double> &frecencies, const std::atomic_bool &cancelled);

    Q_INVOKABLE void search(const QString &text, const QString &open, bool hidden);
    /// `text` as a folder typed in `open` (`~`, `./`, `../` resolved as text), or empty.
    Q_INVOKABLE static QString resolve(const QString &text, const QString &open);
    /// `folder`, `file`, or empty when nothing is there.
    Q_INVOKABLE static QString kind(const QString &path);
    Q_INVOKABLE void cancel();

    QStringList folders() const { return m_folders; }
    bool searching() const { return m_searching; }
    QString message() const { return m_message; }

signals:
    void changed();

private:
    Visits *m_visits;
    QStringList m_folders;
    bool m_searching = false;
    QString m_message;
    std::shared_ptr<std::atomic_bool> m_cancelled;
};
