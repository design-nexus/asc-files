// SPDX-License-Identifier: MIT
#pragma once

#include <QByteArray>
#include <QList>
#include <QObject>
#include <QVariantList>

class BackendClient;
#include "ColumnsModel.h"
#include "FolderModel.h"

/// One tab's browsing: the folder or page it shows, with back and forward history and the
/// breadcrumb. Besides folders, a tab can show a page of its own (Overview, Recent, Favorites,
/// Trash); pages take their place in history like folders do. Paths are kept as raw bytes, so
/// history works for names that are not UTF-8.
class Browser : public QObject {
    Q_OBJECT
    Q_PROPERTY(Page page READ page NOTIFY locationChanged)
    /// How the folder is shown (0 List, 1 Icons, 2 Columns), remembered for each folder.
    Q_PROPERTY(int view READ view WRITE setView NOTIFY viewChanged)
    Q_PROPERTY(FolderModel *folderModel READ folderModel CONSTANT)
    /// The columns Columns view shows; kept up to date while that view is chosen.
    Q_PROPERTY(ColumnsModel *columns READ columns CONSTANT)
    Q_PROPERTY(bool canGoBack READ canGoBack NOTIFY historyChanged)
    Q_PROPERTY(bool canGoForward READ canGoForward NOTIFY historyChanged)
    Q_PROPERTY(bool canGoUp READ canGoUp NOTIFY locationChanged)
    Q_PROPERTY(QVariantList crumbs READ crumbs NOTIFY locationChanged)
    Q_PROPERTY(QString title READ title NOTIFY locationChanged)
    Q_PROPERTY(QString location READ location NOTIFY locationChanged)
public:
    enum Page { Folder, Overview, Recent, Favorites, Trash };
    Q_ENUM(Page)

    explicit Browser(BackendClient *backend, QObject *parent = nullptr);

    Page page() const { return m_here.page; }
    int view() const { return m_view; }
    void setView(int view);
    /// Sorts the folder by `key`; the key it is sorted by already, it turns the other way.
    /// Remembered for the folder.
    Q_INVOKABLE void sortBy(const QString &key);
    /// Shows a page other than a folder.
    Q_INVOKABLE void goPage(Page page);
    /// Where the tab is, as text a session can be restored from: a path, or `page:<name>`.
    QString saved() const;
    Q_INVOKABLE void restore(const QString &saved);

    FolderModel *folderModel() const { return m_folders; }
    ColumnsModel *columns() const { return m_columns; }
    /// Goes into `name` (raw bytes) in the folder at `path`; Columns view's way down.
    void goInto(const QByteArray &path, const QByteArray &name);
    /// Goes into the folder at `row` of a column's listing.
    Q_INVOKABLE void goColumnRow(FolderModel *column, int row);
    QByteArray rawLocation() const;
    QString location() const;
    QString title() const;
    bool canGoBack() const { return !m_back.isEmpty(); }
    bool canGoForward() const { return !m_forward.isEmpty(); }
    bool canGoUp() const;
    /// The breadcrumb: `[{label, home}]` from the top down; under Home the first crumb is Home.
    QVariantList crumbs() const;

    /// Goes to `path`, given as raw bytes, adding the current folder to the back history.
    void go(const QByteArray &path);
    Q_INVOKABLE void goPath(const QString &path);
    /// Goes into the folder at `row` of the listing, by its raw name.
    Q_INVOKABLE void goRow(int row);
    /// Goes to the folder a crumb names.
    Q_INVOKABLE void goCrumb(int index);
    /// Goes to a path the user typed: `~` is Home, and a relative path is taken from the current
    /// folder. False, and nothing changes, when the text names no path.
    Q_INVOKABLE bool goTyped(const QString &text);
    Q_INVOKABLE void back();
    Q_INVOKABLE void forward();
    Q_INVOKABLE void up();

signals:
    void historyChanged();
    void locationChanged();
    void viewChanged();

private:
    struct Place {
        Page page = Folder;
        QByteArray path;
        bool operator==(const Place &other) const { return page == other.page && path == other.path; }
    };
    void go(const Place &place);
    void show(const Place &place);
    /// The raw paths the crumbs stand for, in the same order as crumbs().
    QList<QByteArray> crumbPaths() const;

    FolderModel *m_folders;
    ColumnsModel *m_columns;
    void updateColumns();
    Place m_here;
    QList<Place> m_back;
    QList<Place> m_forward;
    int m_view = 0;
    /// Keeps this folder's view and sort for next time.
    void rememberChoice();
};
