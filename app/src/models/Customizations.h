// SPDX-License-Identifier: MIT
#pragma once

#include <QHash>
#include <QObject>
#include <QStringList>
#include <QVariantList>

/// Folders' and files' own colours and icons, as legacy's Customize gave them: one of seven
/// colours or any (`#rrggbb`), and one of sixteen icons or an emoji. Kept by path in
/// `$XDG_CONFIG_HOME/asc-files/customized.json`; an item renamed or moved leaves its choice
/// behind, as legacy's did. One is shared by the app.
class Customizations : public QObject {
    Q_OBJECT
    /// The colours on offer: `[{name, label, hex}]`.
    Q_PROPERTY(QVariantList colors READ colors CONSTANT)
    /// The icons on offer: `[{name, label, path}]` (16×16 path data).
    Q_PROPERTY(QVariantList icons READ icons CONSTANT)
public:
    static Customizations &shared();
    /// Makes it the `Customizations` singleton of `import AscFiles.System`.
    static void registerType();
    static QString filePath();

    QVariantList colors() const;
    QVariantList icons() const;

    /// `path`'s colour as `#rrggbb`, or empty.
    Q_INVOKABLE QString colorOf(const QString &path) const;
    /// `path`'s icon: path data for a chosen icon, `emoji:…` for an emoji, or empty.
    Q_INVOKABLE QString iconOf(const QString &path) const;
    /// The choices as kept (`red`, `#123456`; `documents`, `emoji:🎵`), for the dialog.
    Q_INVOKABLE QString chosenColor(const QString &path) const { return m_colors.value(path); }
    Q_INVOKABLE QString chosenIcon(const QString &path) const { return m_icons.value(path); }
    /// Gives `paths` a colour (a name on offer or `#rrggbb`; empty takes it away).
    Q_INVOKABLE void setColor(const QStringList &paths, const QString &color);
    /// Gives `paths` an icon (a name on offer or `emoji:…`; empty takes it away).
    Q_INVOKABLE void setIcon(const QStringList &paths, const QString &icon);
    /// Takes `paths`' colour and icon away.
    Q_INVOKABLE void reset(const QStringList &paths);

signals:
    void changed(const QStringList &paths);

private:
    Customizations();
    void save() const;

    QHash<QString, QString> m_colors;
    QHash<QString, QString> m_icons;
};
