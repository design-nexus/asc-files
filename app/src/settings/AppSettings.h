// SPDX-License-Identifier: MIT
#pragma once

#include <QObject>
#include <QSettings>
#include <QStringList>

/// Files' preferences, as legacy Files offered them, kept in the settings file
/// (`$XDG_CONFIG_HOME/asc-files/asc-files.ini`). One is shared by the app and every window: a
/// change shows everywhere at once. Each setting here does something; legacy's settings for
/// features Files doesn't have yet come with those features.
class AppSettings : public QObject {
    Q_OBJECT
    // General
    /// The folder a window opens on when given none; empty for Home.
    Q_PROPERTY(QString startFolder READ startFolder WRITE setStartFolder NOTIFY changed)
    /// Whether the last session's tabs come back (when it left more than one).
    Q_PROPERTY(bool restoreTabs READ restoreTabs WRITE setRestoreTabs NOTIFY changed)
    /// How modified times read: "relative" (5m ago, Tuesday), "iso" (2026-11-24 14:05), "long".
    Q_PROPERTY(QString dateFormat READ dateFormat WRITE setDateFormat NOTIFY changed)
    /// Whether the header shows Minimize and Maximize beside Close.
    Q_PROPERTY(bool windowButtons READ windowButtons WRITE setWindowButtons NOTIFY changed)
    // Browsing
    /// Whether each folder keeps the view and sort chosen there.
    Q_PROPERTY(bool rememberFolderViews READ rememberFolderViews WRITE setRememberFolderViews NOTIFY changed)
    /// The view and sort of folders without their own.
    Q_PROPERTY(int defaultView READ defaultView WRITE setDefaultView NOTIFY changed)
    Q_PROPERTY(QString defaultSort READ defaultSort WRITE setDefaultSort NOTIFY changed)
    Q_PROPERTY(bool defaultDescending READ defaultDescending WRITE setDefaultDescending NOTIFY changed)
    /// `clicks()` for every view, by `"<view>/files"` and `"<view>/folders"`, for bindings.
    Q_PROPERTY(QVariantMap clickCounts READ clickCounts NOTIFY changed)
    /// Whether any folder keeps its own view or sort.
    Q_PROPERTY(bool hasFolderViews READ hasFolderViews NOTIFY changed)
    /// List view's columns after Name, in order: `[{key, width}]` (mode, size, kind, modified).
    Q_PROPERTY(QVariantList listColumns READ listColumns WRITE setListColumns NOTIFY changed)
    // Search
    /// Whether typing in a folder filters it.
    Q_PROPERTY(bool typeToFilter READ typeToFilter WRITE setTypeToFilter NOTIFY changed)
    /// How long a search goes on, in seconds, and how many entries it looks at; 0: no limit.
    Q_PROPERTY(int searchSeconds READ searchSeconds WRITE setSearchSeconds NOTIFY changed)
    Q_PROPERTY(int searchEntries READ searchEntries WRITE setSearchEntries NOTIFY changed)
    /// Folders left out of every search, by name or path, as the backend keeps them.
    Q_PROPERTY(QStringList searchExclusions READ searchExclusions WRITE setSearchExclusions NOTIFY changed)
    // Transfers
    /// What a drop onto another drive does: "copy", "move" or "ask" (legacy's default).
    Q_PROPERTY(QString crossDriveDrop READ crossDriveDrop WRITE setCrossDriveDrop NOTIFY changed)
    /// Whether the folder dropped into is shown after the drop.
    Q_PROPERTY(bool openAfterDrop READ openAfterDrop WRITE setOpenAfterDrop NOTIFY changed)
    // Previews
    /// "popup" over the window or "panel" beside the list.
    Q_PROPERTY(QString previewStyle READ previewStyle WRITE setPreviewStyle NOTIFY changed)
    Q_PROPERTY(bool showThumbnails READ showThumbnails WRITE setShowThumbnails NOTIFY changed)
    /// Whether audio and video previews start playing as they open (legacy's default: no).
    Q_PROPERTY(bool autoplayMedia READ autoplayMedia WRITE setAutoplayMedia NOTIFY changed)
    /// The previews' volume, 0–1, as last set.
    Q_PROPERTY(double mediaVolume READ mediaVolume WRITE setMediaVolume NOTIFY changed)
    /// The preview panel's width (260–900 px).
    Q_PROPERTY(int previewWidth READ previewWidth WRITE setPreviewWidth NOTIFY changed)
public:
    /// The one the app shares.
    static AppSettings &shared();
    /// Makes it the `AppSettings` singleton of `import AscFiles.System`.
    static void registerType();

    QString startFolder() const;
    void setStartFolder(const QString &folder);
    /// The start folder if it still exists, else Home.
    Q_INVOKABLE QString startFolderOrHome() const;
    bool restoreTabs() const;
    void setRestoreTabs(bool restore);
    QString dateFormat() const;
    void setDateFormat(const QString &format);
    bool windowButtons() const;
    void setWindowButtons(bool shown);

    /// Whether a click (1) or a double click (2) opens files (`files`) or folders in `view`
    /// (0 List, 1 Icons, 2 Columns). As legacy: files take two everywhere, folders one in Columns.
    Q_INVOKABLE int clicks(int view, bool files) const;
    Q_INVOKABLE void setClicks(int view, bool files, int count);
    QVariantMap clickCounts() const;

    bool rememberFolderViews() const;
    void setRememberFolderViews(bool remember);
    int defaultView() const;
    void setDefaultView(int view);
    QString defaultSort() const;
    void setDefaultSort(const QString &key);
    bool defaultDescending() const;
    void setDefaultDescending(bool descending);
    /// Every folder back to the default view and sort.
    Q_INVOKABLE void forgetFolderViews();
    bool hasFolderViews() const;

    QVariantList listColumns() const;
    /// Keeps `columns` (each known key once, widths held to 40–400; ones left out follow).
    void setListColumns(const QVariantList &columns);
    Q_INVOKABLE void resetListColumns();
    /// A column's width when it is reset.
    Q_INVOKABLE int defaultColumnWidth(const QString &key) const;

    bool typeToFilter() const;
    void setTypeToFilter(bool filters);
    /// Legacy's choices: 10 s to 15 min, or none (0); others go up to the next.
    int searchSeconds() const;
    void setSearchSeconds(int seconds);
    /// Legacy's choices: 200,000 to 5,000,000, or none (0).
    int searchEntries() const;
    void setSearchEntries(int entries);
    QStringList searchExclusions() const;
    void setSearchExclusions(const QStringList &rules);
    Q_INVOKABLE void removeSearchExclusion(const QString &rule);
    QString crossDriveDrop() const;
    void setCrossDriveDrop(const QString &choice);
    bool openAfterDrop() const;
    void setOpenAfterDrop(bool opens);

    QString previewStyle() const;
    void setPreviewStyle(const QString &style);
    bool showThumbnails() const;
    int previewWidth() const;
    bool autoplayMedia() const;
    void setAutoplayMedia(bool autoplay);
    double mediaVolume() const;
    void setMediaVolume(double volume);
    void setPreviewWidth(int width);
    void setShowThumbnails(bool shown);

    /// A modified time in the chosen format, relative to `now`.
    QString modifiedText(const QDateTime &when, const QDateTime &now) const;
    /// A date in full ("Tuesday, 24 November 2026, 14:05"): the long date and the short time,
    /// without a time zone's name (which would ask the system for it).
    static QString fullDate(const QDateTime &when);
    /// What each date format looks like, for the setting: a time five minutes ago.
    Q_INVOKABLE QString dateExample(const QString &format) const;
    /// For About and bug reports: `{version, commit, qt, sandbox, settingsFile}`.
    Q_INVOKABLE QVariantMap about() const;
    /// The same as text, to paste into a report.
    Q_INVOKABLE void copyAbout() const;

signals:
    void changed();
    /// Dates read differently: lists word them again.
    void dateFormatChanged();

private:
    AppSettings();
    QVariant value(const char *key, const QVariant &fallback) const;
    QVariantList normalizedColumns(const QVariantList &given) const;
    void set(const char *key, const QVariant &value);
    mutable QSettings m_settings;
};
