// SPDX-License-Identifier: MIT
#pragma once

#include <QAbstractListModel>
#include <QHash>
#include <QJsonArray>
#include <QList>
#include <QSet>

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
    Q_PROPERTY(int selectionCount READ selectionCount NOTIFY selectionChanged)
    /// Originals an interrupted operation set aside here, by name, for the leftovers notice.
    Q_PROPERTY(QStringList leftovers READ leftovers NOTIFY leftoversChanged)
    Q_PROPERTY(QString sortKey READ sortKey NOTIFY sortChanged)
    Q_PROPERTY(bool sortDescending READ sortDescending NOTIFY sortChanged)
    Q_PROPERTY(bool foldersFirst READ foldersFirst NOTIFY sortChanged)
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
        SelectedRole,
        /// The `image://thumbnail/` id of an image Files can make a thumbnail of; empty otherwise.
        ThumbnailRole,
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

    /// The path of the entry at `row`, as text, or empty while its page is not here.
    Q_INVOKABLE QString pathAt(int row) const;
    /// The row of the entry shown as `name` among the rows that are here, or -1.
    Q_INVOKABLE int rowOfName(const QString &name) const;
    /// Whether `row` is a folder, or a link to one; false while its page is not here.
    Q_INVOKABLE bool isFolder(int row) const;
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
    QString sortKey() const { return m_sortKey; }
    bool sortDescending() const { return m_sortDescending; }
    bool foldersFirst() const { return m_foldersFirst; }
    /// Orders the listing by `key` ("name", "size" or "modified"); the backend reads the folder
    /// again in that order, keeping what it knows.
    Q_INVOKABLE void setSort(const QString &key, bool descending, bool foldersFirst = true);
    /// Opens `path` already ordered by `key`, so the folder is read once.
    void open(const QByteArray &path, const QString &key, bool descending);
    /// Lists by modified time, newest first, without folders first (for "what's new").
    void setNewestFirst(bool newest);
    bool isLoaded(int row) const;
    int pagesFetched() const { return m_pagesFetched; }

    // Selection, kept by name so it outlasts the folder being read again. Ranges over rows not
    // here yet fill in as their pages arrive; "all" is kept as all-but-these, so selecting
    // everything in a folder of any size needs none of its names.
    int selectionCount() const;
    Q_INVOKABLE bool isSelected(int row) const;
    /// Selects `row` alone and makes it the anchor for ranges.
    Q_INVOKABLE void select(int row);
    /// Adds or removes `row` and makes it the anchor.
    Q_INVOKABLE void toggle(int row);
    /// Selects the rows from the anchor to `row`, and only those.
    Q_INVOKABLE void extendTo(int row);
    /// Selects `rows` (a rubber band's), added to the selection or in place of it.
    Q_INVOKABLE void selectRows(const QList<int> &rows, bool add);
    Q_INVOKABLE void selectAll();
    Q_INVOKABLE void clearSelection();

    /// Renames the item at `row` to `name`, as typed: refused, with the reason, where its drive
    /// would store another name or the name is taken. The item stays selected under its new name.
    Q_INVOKABLE void rename(int row, const QString &name);
    /// The names selected among the rows that are here (all of them unless "all" is selected in
    /// a folder not fully read).
    QList<QByteArray> selectedNames() const;
    bool allSelected() const { return m_allSelected; }
    QStringList leftovers() const;
    QList<QByteArray> rawLeftovers() const { return m_leftovers; }

    int rowCount(const QModelIndex &parent = {}) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

signals:
    void folderChanged();
    void loadingChanged();
    void errorChanged();
    void generationChanged();
    void showHiddenChanged();
    void sortChanged();
    /// The folder is listed and its count known.
    void opened(int count);
    void selectionChanged();
    void leftoversChanged();
    /// An operation on this folder was refused; `message` says why, for the user.
    void failed(const QString &message);
    void renamed(const QString &name);
    /// The folder went (deleted, moved away or unmounted); the model moved to `nearest`.
    void folderLeft(const QString &folder, const QString &nearest, bool unmounted);

private:
    void close();
    void request(int page) const;
    void notification(const QString &method, const QJsonObject &params);
    /// The folder was read again: `count` rows at `generation`.
    void reread(qint64 generation, int count);
    void setLoading(bool loading);
    /// Asks the backend to order the open listing as the model is set.
    void reorder();
    QJsonObject sortParams() const;
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
    QString m_sortKey = QStringLiteral("name");
    bool m_sortDescending = false;
    bool m_foldersFirst = true;
    QString m_error;
    QString m_errorKind;
    mutable QHash<int, QJsonArray> m_pages;
    mutable QList<int> m_order;
    /// Pages asked for, with the generation each was asked for in.
    mutable QHash<int, qint64> m_inFlight;
    mutable int m_pagesFetched = 0;

    void selectionTouched();
    /// Adds the names of the rows of `first`–`last` that are here; returns the rows that are not.
    QList<int> addNames(int first, int last);
    /// Adds pending range rows whose page has just arrived.
    void resolvePending(int page);
    QSet<QByteArray> m_selected;
    bool m_allSelected = false;
    /// Rows a range named before their pages arrived, in the generation it was made in.
    QSet<int> m_pendingRows;
    int m_anchor = -1;
    QList<QByteArray> m_leftovers;
    /// Keeps what a listing says interrupted operations left; old probes are swept away.
    void takeLeftovers(const QJsonValue &names);
};
