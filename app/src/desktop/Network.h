// SPDX-License-Identifier: MIT
#pragma once

#include <QHash>
#include <QObject>
#include <QVariantList>

typedef struct _GMountOperation GMountOperation;
typedef struct _GVolumeMonitor GVolumeMonitor;

/// Network locations (SFTP, SMB, WebDAV, FTP, …) through GIO and gvfs, as legacy reached them: a
/// server is mounted, asking for a password where it wants one, and opened as the folder gvfs
/// shows it as (under `$XDG_RUNTIME_DIR/gvfs`), which Files browses like any other.
class Network : public QObject {
    Q_OBJECT
    /// `[{name, path, uri}]`: the network locations connected now.
    Q_PROPERTY(QVariantList mounts READ mounts NOTIFY changed)
public:
    explicit Network(QObject *parent = nullptr);
    ~Network() override;

    QVariantList mounts() const { return m_mounts; }

    /// Whether `text` names a server address (`scheme://…`) rather than a folder.
    Q_INVOKABLE static bool isAddress(const QString &text);
    /// Connects to `address` (or finds it connected) and says where it is: `opened`.
    Q_INVOKABLE void connectTo(const QString &address);
    Q_INVOKABLE void disconnectFrom(const QString &uri);
    /// Answers the password `asked` for: a user, a password, a domain, anonymously, and how long
    /// to remember it (`never`, `session`, `always`); `cancel` gives up connecting.
    Q_INVOKABLE void answerPassword(int asked, const QString &user, const QString &password, const QString &domain, bool anonymous,
                                    const QString &remember);
    Q_INVOKABLE void answerQuestion(int asked, int choice);
    Q_INVOKABLE void cancel(int asked);

signals:
    void changed();
    /// Connected: `path` is the folder to show.
    void opened(const QString &path);
    void failed(const QString &message);
    /// A server wants a password: answer with `answerPassword` or `cancel`.
    void passwordWanted(int asked, const QString &message, const QString &user, const QString &domain, bool needsUser, bool needsDomain,
                        bool anonymousAllowed, bool canRemember);
    void questionAsked(int asked, const QString &message, const QStringList &choices);

public:
    /// Reads the network locations connected now.
    void refresh();

private:
    int ask(GMountOperation *operation);
    GVolumeMonitor *m_monitor = nullptr;
    QVariantList m_mounts;
    QHash<int, GMountOperation *> m_asking;
    int m_next = 0;

public:
    // For GIO's callbacks.
    void finishConnect(void *file, void *result);
    void askPassword(GMountOperation *operation, const char *message, const char *user, const char *domain, int flags);
    void askQuestion(GMountOperation *operation, const char *message, const char **choices);
    GMountOperation *operation();
};
