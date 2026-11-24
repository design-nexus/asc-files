// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QHash>
#include <QJsonArray>
#include <QList>

class BackendClient;
struct BackendError;

/// One folder's entries, served by a backend listing (`list.*`). The count is known as soon as the
/// folder is read; rows arrive a page at a time as the view asks for them, and pages are dropped
/// beyond a bound, so a folder of any size costs the same memory.
///
/// Every read of the folder is a new generation. When the folder changes outside, the backend
/// reads it again and says so; the model drops its pages and tells the view which rows changed,
/// keeping its place rather than resetting. A deleted or unmounted folder takes the model to the
/// nearest folder left above it.
class FolderModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(QString folder READ folder NOTIFY folderChanged)
    Q_PROPERTY(QString folderName READ folderName NOTIFY folderChanged)
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
    Q_PROPERTY(QString errorKind READ errorKind NOTIFY errorChanged)
    Q_PROPERTY(bool followsChanges READ followsChanges NOTIFY folderChanged)
    Q_PROPERTY(qint64 generation READ generation NOTIFY generationChanged)
    Q_PROPERTY(bool showHidden READ showHidden WRITE setShowHidden NOTIFY showHiddenChanged)
public:
    enum Role {
        NameRole = Qt::UserRole + 1,
        KindRole,
        KindTextRole,
        IsFolderRole,
        HiddenRole,
        SizeRole,
        SizeTextRole,
        ModifiedRole,
        ModifiedTextRole,
        ModeOctalRole,
        ModeSymbolicRole,
        GoneRole,
        LoadedRole,
    };
    Q_ENUM(Role)
    static constexpr int PageSize = 200;
    /// Pages kept; the oldest is dropped beyond this.
    static constexpr int MaxPages = 64;

    explicit FolderModel(BackendClient *backend, QObject *parent = nullptr);
    ~FolderModel() override;

    /// Lists the folder at `path`, given as raw bytes.
    void open(const QByteArray &path);
    /// Lists the folder at a path typed or chosen as text.
    Q_INVOKABLE void openPath(const QString &path);
    /// Lists the folder at `row`, by its raw name, so a name that is not UTF-8 still opens.
    Q_INVOKABLE void openRow(int row);
    /// Lists the folder above this one.
    Q_INVOKABLE void openParent();
    Q_INVOKABLE void refresh();

    QByteArray rawFolder() const { return m_folder; }
    /// The raw name of `row`, or empty while its page is not here.
    QByteArray rawName(int row) const;
    QString folder() const;
    QString folderName() const;
    bool loading() const { return m_loading; }
    QString error() const { return m_error; }
    QString errorKind() const { return m_errorKind; }
    bool followsChanges() const { return m_followsChanges; }
    qint64 generation() const { return m_generation; }
    bool showHidden() const { return m_showHidden; }
    void setShowHidden(bool show);
    bool isLoaded(int row) const;
    int pagesFetched() const { return m_pagesFetched; }

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

signals:
    void folderChanged();
    void loadingChanged();
    void errorChanged();
    void generationChanged();
    void showHiddenChanged();
    /// The folder is listed and its count known.
    void opened(int count);
    /// The folder went (deleted, moved away or unmounted); the model moved to `nearest`.
    void folderLeft(const QString &folder, const QString &nearest, bool unmounted);

private:
    void close();
    void request(int page) const;
    void notification(const QString &method, const QJsonObject &params);
    /// The folder was read again: `count` rows at `generation`.
    void reread(qint64 generation, int count);
    void setLoading(bool loading);
    void setError(const BackendError &error);

    BackendClient *m_backend;
    QByteArray m_folder;
    qint64 m_listing = -1;
    /// Bumped by each open, so a reply meant for an earlier one is dropped.
    quint64 m_opening = 0;
    qint64 m_generation = 0;
    int m_count = 0;
    bool m_loading = false;
    bool m_followsChanges = false;
    bool m_showHidden = false;
    QString m_error;
    QString m_errorKind;
    mutable QHash<int, QJsonArray> m_pages;
    mutable QList<int> m_order;
    /// Pages asked for, with the generation each was asked for in.
    mutable QHash<int, qint64> m_inFlight;
    mutable int m_pagesFetched = 0;
};
