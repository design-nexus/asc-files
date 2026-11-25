// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QJsonObject>
#include <QVariantList>

class BackendClient;

/// Custom actions (the backend's `actions.*`): what is on offer for a selection, running them,
/// and their runs as rows (newest first) for the jobs list; also the actions folder for
/// Settings (`actions`, `failures`) and its edits. One is shared by the app's windows.
class ActionsModel : public QAbstractListModel {
    Q_OBJECT
    /// Every action: `[{id, name, description, icon, enabled, menu, runtime, mode, confirm,
    /// available, reason}]`, and the folders that couldn't be read `[{directory, reason}]`.
    Q_PROPERTY(QVariantList actions READ actions NOTIFY listChanged)
    Q_PROPERTY(QVariantList failures READ failures NOTIFY listChanged)
    /// What the selection last asked about is offered, in menu order (entries as `actions`).
    Q_PROPERTY(QVariantList offered READ offered NOTIFY offeredChanged)
    Q_PROPERTY(int running READ running NOTIFY runsChanged)
    Q_PROPERTY(int count READ rowCount NOTIFY runsChanged)
public:
    enum Role {
        JobRole = Qt::UserRole + 1,
        NameRole,
        StatusRole,  ///< "queued", "running", "cancelling", "succeeded", "failed", "cancelled"
        ActiveRole,
        FractionRole, ///< 0–1, or -1 while unknown
        StepRole,     ///< What the script says it is doing, or a per-item count.
        MessageRole,
        LogRole,
        CreatedRole,
        ElapsedRole,
    };
    Q_ENUM(Role)

    explicit ActionsModel(BackendClient *backend, QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    QVariantList actions() const { return m_actions; }
    QVariantList failures() const { return m_failures; }
    QVariantList offered() const { return m_offered; }
    int running() const;

    /// Reads the actions folder again (Settings).
    Q_INVOKABLE void reload();
    /// Asks what `paths` are offered; `offered` follows.
    Q_INVOKABLE void offer(const QStringList &paths);
    /// Runs `id` on `paths`, invoked from the folder `parent` (`background`: on the folder itself,
    /// from its empty space).
    Q_INVOKABLE void run(const QString &id, const QStringList &paths, const QString &parent, bool background = false);
    Q_INVOKABLE void cancel(qint64 job);
    /// Forgets the finished runs.
    Q_INVOKABLE void clearFinished();

    // Settings' edits; each reads the folder again after, and says why when one fails.
    Q_INVOKABLE void setEnabled(const QString &id, bool enabled);
    Q_INVOKABLE void remove(const QString &id);
    Q_INVOKABLE void duplicate(const QString &id);
    Q_INVOKABLE void importFrom(const QString &folder);
    Q_INVOKABLE void exportTo(const QString &id, const QString &folder);
    /// Reads an action for the editor; `read` follows with `{definition, script}`.
    Q_INVOKABLE void read(const QString &id);
    /// Saves the editor's action (`create`: a new one); `saved` or `failed` follows.
    Q_INVOKABLE void write(const QVariantMap &definition, const QVariantMap &script, bool create);
    /// The recipe library; `examplesRead` follows.
    Q_INVOKABLE void readExamples();

    /// An input for the backend: the path, whether a folder, its type.
    static QJsonObject input(const QString &path);

signals:
    void listChanged();
    void offeredChanged();
    void runsChanged();
    void failed(const QString &message);
    /// A run was queued (the window shows the jobs).
    void started(qint64 job);
    void readDone(const QVariantMap &action);
    void saved(const QString &id);
    void examplesRead(const QVariantList &examples);
    /// An export went to `path`.
    void exported(const QString &path);

private:
    void notification(const QString &method, const QJsonObject &params);
    void edit(const QString &method, const QJsonObject &params);

    BackendClient *m_backend;
    QVariantList m_actions;
    QVariantList m_failures;
    QVariantList m_offered;
    /// Asked for this selection; an answer for an older one is dropped.
    quint64 m_offerSerial = 0;
    QList<QJsonObject> m_runs;
};
