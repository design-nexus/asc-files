// SPDX-License-Identifier: MIT
#include "AppSettings.h"

#include "../models/FolderViews.h"
#include "../render/Sandbox.h"

#include <QClipboard>
#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QLocale>
#include <QQmlEngine>
#include <QtQml/qqml.h>

namespace {

const QStringList DateFormats{QStringLiteral("relative"), QStringLiteral("iso"), QStringLiteral("long")};
const QList<int> SearchSeconds{10, 30, 60, 300, 900};
const QList<int> SearchEntries{200'000, 500'000, 1'000'000, 5'000'000};

/// A limit as one of `choices`: an unlisted one goes up to the next; 0, or past the largest,
/// is no limit.
int chosenLimit(int value, const QList<int> &choices) {
    for (const int choice : choices) {
        if (value > 0 && value <= choice) {
            return choice;
        }
    }
    return 0;
}

const QStringList SortKeys{QStringLiteral("name"), QStringLiteral("size"), QStringLiteral("modified"), QStringLiteral("type")};

QString absolute(const QDateTime &when, const QString &format) {
    if (format == QLatin1String("long")) {
        return AppSettings::fullDate(when);
    }
    return when.toString(QStringLiteral("yyyy-MM-dd HH:mm"));
}

} // namespace

AppSettings::AppSettings()
    : m_settings(QSettings::IniFormat, QSettings::UserScope, QStringLiteral("asc-files"), QStringLiteral("asc-files")) {}

AppSettings &AppSettings::shared() {
    static AppSettings settings;
    return settings;
}

void AppSettings::registerType() {
    qmlRegisterSingletonType<AppSettings>("AscFiles.System", 1, 0, "AppSettings", [](QQmlEngine *, QJSEngine *) {
        AppSettings *settings = &shared();
        // Shared by every engine; none owns it.
        QQmlEngine::setObjectOwnership(settings, QQmlEngine::CppOwnership);
        return settings;
    });
}

QVariant AppSettings::value(const char *key, const QVariant &fallback) const {
    return m_settings.value(QLatin1String(key), fallback);
}

void AppSettings::set(const char *key, const QVariant &value) {
    if (m_settings.value(QLatin1String(key)) == value) {
        return;
    }
    m_settings.setValue(QLatin1String(key), value);
    m_settings.sync();
    emit changed();
}

QString AppSettings::startFolder() const { return value("general/startFolder", QString()).toString(); }
void AppSettings::setStartFolder(const QString &folder) { set("general/startFolder", folder); }

QString AppSettings::startFolderOrHome() const {
    const QString folder = startFolder();
    return !folder.isEmpty() && QFileInfo(folder).isDir() ? folder : QDir::homePath();
}

bool AppSettings::restoreTabs() const { return value("general/restoreTabs", true).toBool(); }
void AppSettings::setRestoreTabs(bool restore) { set("general/restoreTabs", restore); }

QString AppSettings::dateFormat() const {
    const QString format = value("general/dateFormat", DateFormats.first()).toString();
    return DateFormats.contains(format) ? format : DateFormats.first();
}

void AppSettings::setDateFormat(const QString &format) {
    if (DateFormats.contains(format) && format != dateFormat()) {
        set("general/dateFormat", format);
        emit dateFormatChanged();
    }
}

bool AppSettings::windowButtons() const { return value("general/windowButtons", false).toBool(); }
void AppSettings::setWindowButtons(bool shown) { set("general/windowButtons", shown); }

int AppSettings::clicks(int view, bool files) const {
    const int fallback = files || view != FolderViews::Columns ? 2 : 1;
    const int count = m_settings.value(QStringLiteral("clicks/%1/%2").arg(view).arg(files ? "files" : "folders"), fallback).toInt();
    return count == 1 ? 1 : 2;
}

void AppSettings::setClicks(int view, bool files, int count) {
    const QString key = QStringLiteral("clicks/%1/%2").arg(view).arg(files ? "files" : "folders");
    set(key.toLatin1().constData(), count == 1 ? 1 : 2);
}

QVariantMap AppSettings::clickCounts() const {
    QVariantMap counts;
    for (int view = FolderViews::List; view <= FolderViews::Columns; ++view) {
        counts.insert(QStringLiteral("%1/files").arg(view), clicks(view, true));
        counts.insert(QStringLiteral("%1/folders").arg(view), clicks(view, false));
    }
    return counts;
}

bool AppSettings::rememberFolderViews() const { return value("browsing/rememberFolderViews", true).toBool(); }
void AppSettings::setRememberFolderViews(bool remember) { set("browsing/rememberFolderViews", remember); }

int AppSettings::defaultView() const { return qBound(0, value("browsing/defaultView", int(FolderViews::List)).toInt(), 2); }
void AppSettings::setDefaultView(int view) { set("browsing/defaultView", qBound(0, view, 2)); }

QString AppSettings::defaultSort() const {
    const QString key = value("browsing/defaultSort", SortKeys.first()).toString();
    return SortKeys.contains(key) ? key : SortKeys.first();
}

void AppSettings::setDefaultSort(const QString &key) {
    if (SortKeys.contains(key)) {
        set("browsing/defaultSort", key);
    }
}

bool AppSettings::defaultDescending() const { return value("browsing/defaultDescending", false).toBool(); }
void AppSettings::setDefaultDescending(bool descending) { set("browsing/defaultDescending", descending); }

void AppSettings::forgetFolderViews() {
    FolderViews::shared().forgetAll();
    emit changed();
}

bool AppSettings::hasFolderViews() const { return FolderViews::shared().count() > 0; }

bool AppSettings::typeToFilter() const { return value("search/typeToFilter", true).toBool(); }
void AppSettings::setTypeToFilter(bool filters) { set("search/typeToFilter", filters); }

int AppSettings::searchSeconds() const { return chosenLimit(value("search/seconds", 10).toInt(), SearchSeconds); }
void AppSettings::setSearchSeconds(int seconds) { set("search/seconds", chosenLimit(seconds, SearchSeconds)); }

int AppSettings::searchEntries() const { return chosenLimit(value("search/entries", 200'000).toInt(), SearchEntries); }
void AppSettings::setSearchEntries(int entries) { set("search/entries", chosenLimit(entries, SearchEntries)); }

QStringList AppSettings::searchExclusions() const { return value("search/exclusions", QStringList()).toStringList(); }

void AppSettings::setSearchExclusions(const QStringList &rules) {
    QStringList kept;
    for (const QString &rule : rules) {
        if (!rule.isEmpty() && !kept.contains(rule)) {
            kept.append(rule);
        }
    }
    set("search/exclusions", kept);
}

void AppSettings::removeSearchExclusion(const QString &rule) {
    QStringList rules = searchExclusions();
    rules.removeAll(rule);
    setSearchExclusions(rules);
}

QString AppSettings::crossDriveDrop() const {
    const QString choice = value("transfers/crossDriveDrop", QStringLiteral("ask")).toString();
    return choice == QLatin1String("copy") || choice == QLatin1String("move") ? choice : QStringLiteral("ask");
}

void AppSettings::setCrossDriveDrop(const QString &choice) {
    set("transfers/crossDriveDrop", choice == QLatin1String("copy") || choice == QLatin1String("move") ? choice : QStringLiteral("ask"));
}

bool AppSettings::openAfterDrop() const { return value("transfers/openAfterDrop", false).toBool(); }
void AppSettings::setOpenAfterDrop(bool opens) { set("transfers/openAfterDrop", opens); }

QString AppSettings::previewStyle() const {
    return value("preview/style", QStringLiteral("popup")).toString() == QLatin1String("panel") ? QStringLiteral("panel") : QStringLiteral("popup");
}

void AppSettings::setPreviewStyle(const QString &style) { set("preview/style", style == QLatin1String("panel") ? style : QStringLiteral("popup")); }

bool AppSettings::showThumbnails() const { return value("previews/thumbnails", true).toBool(); }
void AppSettings::setShowThumbnails(bool shown) { set("previews/thumbnails", shown); }

QString AppSettings::modifiedText(const QDateTime &when, const QDateTime &now) const {
    if (!when.isValid()) {
        return {};
    }
    const QString format = dateFormat();
    if (format != QLatin1String("relative")) {
        return absolute(when, format);
    }
    // Hours ago today, the weekday this week, else the date.
    const qint64 seconds = when.secsTo(now);
    if (seconds < 0) {
        return QLocale().toString(when, QLocale::ShortFormat);
    }
    if (seconds < 60) {
        return tr("Just now");
    }
    if (seconds < 3600) {
        return tr("%1m ago").arg(seconds / 60);
    }
    if (seconds < 86400) {
        return tr("%1h ago").arg(seconds / 3600);
    }
    if (when.date().daysTo(now.date()) < 7) {
        return QLocale().dayName(when.date().dayOfWeek());
    }
    return QLocale().toString(when.date(), QStringLiteral("MMM d, yyyy"));
}

QString AppSettings::fullDate(const QDateTime &when) {
    const QLocale locale;
    return tr("%1, %2").arg(locale.toString(when.date(), QLocale::LongFormat), locale.toString(when.time(), QLocale::ShortFormat));
}

QVariantMap AppSettings::about() const {
    return {{QStringLiteral("version"), QStringLiteral(ASC_FILES_VERSION)},
            {QStringLiteral("commit"), QStringLiteral(ASC_FILES_COMMIT)},
            {QStringLiteral("qt"), QString::fromLatin1(qVersion())},
            {QStringLiteral("sandbox"), Sandbox::sandboxed() ? tr("Isolated (bubblewrap) with limits") : tr("Limits only (bubblewrap unavailable)")},
            {QStringLiteral("settingsFile"), m_settings.fileName()}};
}

void AppSettings::copyAbout() const {
    const QVariantMap info = about();
    QGuiApplication::clipboard()->setText(QStringLiteral("Files %1 (%2)\nQt %3\nPreviews: %4")
                                              .arg(info.value("version").toString(), info.value("commit").toString(),
                                                   info.value("qt").toString(), info.value("sandbox").toString()));
}

QString AppSettings::dateExample(const QString &format) const {
    const QDateTime now = QDateTime::currentDateTime();
    const QDateTime sample = now.addSecs(-300);
    return format == QLatin1String("relative") ? tr("5m ago") : absolute(sample, format);
}
