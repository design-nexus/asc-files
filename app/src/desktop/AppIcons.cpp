// SPDX-License-Identifier: MIT
#include "AppIcons.h"

#include <QFile>
#include <QIcon>
#include <QProcess>
#include <QSettings>
#include <QStandardPaths>
#include <QUrl>

QPixmap AppIconProvider::requestPixmap(const QString &id, QSize *size, const QSize &requestedSize) {
    const QSize wanted = requestedSize.isValid() && !requestedSize.isEmpty() ? requestedSize : QSize(32, 32);
    const QString name = QUrl::fromPercentEncoding(id.toUtf8());
    QIcon icon = name.startsWith(u'/') ? QIcon(name) : QIcon::fromTheme(name);
    if (icon.isNull()) {
        icon = QIcon::fromTheme(QStringLiteral("application-x-executable"));
    }
    const QPixmap pixmap = icon.pixmap(wanted);
    if (size) {
        *size = pixmap.size();
    }
    return pixmap;
}

void AppIconProvider::useDesktopTheme() {
    const QString current = QIcon::themeName();
    if (!current.isEmpty() && current != QLatin1String("hicolor")) {
        return;
    }
    QString theme;
    for (const char *gtk : {"/gtk-4.0/settings.ini", "/gtk-3.0/settings.ini"}) {
        const QString path = QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QLatin1String(gtk);
        if (theme.isEmpty() && QFile::exists(path)) {
            theme = QSettings(path, QSettings::IniFormat).value(QStringLiteral("Settings/gtk-icon-theme-name")).toString();
        }
    }
    if (theme.isEmpty() && !QStandardPaths::findExecutable(QStringLiteral("gsettings")).isEmpty()) {
        QProcess gsettings;
        gsettings.start(QStringLiteral("gsettings"), {QStringLiteral("get"), QStringLiteral("org.gnome.desktop.interface"), QStringLiteral("icon-theme")});
        if (gsettings.waitForFinished(1500) && gsettings.exitCode() == 0) {
            theme = QString::fromUtf8(gsettings.readAllStandardOutput()).trimmed().remove(u'\'');
        }
    }
    if (!theme.isEmpty()) {
        QIcon::setThemeName(theme);
    }
}
