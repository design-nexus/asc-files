// SPDX-License-Identifier: MIT
#pragma once

#include <QQuickImageProvider>

/// Programs' icons, by the name their entry gives (from the icon theme) or a picture's path, as
/// `image://appicon/<name>`.
class AppIconProvider : public QQuickImageProvider {
public:
    AppIconProvider() : QQuickImageProvider(QQuickImageProvider::Pixmap) {}
    QPixmap requestPixmap(const QString &id, QSize *size, const QSize &requestedSize) override;

    /// Uses the desktop's icon theme. Without a platform theme Qt knows none, so it is read as
    /// GTK keeps it: its settings file, else the desktop's settings (`gsettings`).
    static void useDesktopTheme();
};
