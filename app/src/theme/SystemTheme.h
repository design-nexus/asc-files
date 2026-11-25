// SPDX-License-Identifier: MIT
#pragma once

#include <QColor>
#include <QFileSystemWatcher>
#include <QObject>
#include <QVariantMap>

/// The Nyx system theme, as Files shows it. The theme engine writes the resolved theme as one
/// snapshot (`$XDG_RUNTIME_DIR/nyx/theme/system.json`, replaced whole), which every Nyx app reads
/// so none resolves themes itself; this follows it, so a theme switched in Settings reaches Files
/// at once, every colour together. The suite's settings (`~/.config/nyx/theme.toml`) add the
/// text size, the density and the accent chosen for Files. Until a snapshot can be read, Nyx Dark
/// is used, so every token always has a value.
class SystemTheme : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap colors READ colors NOTIFY changed)
    Q_PROPERTY(QVariantMap swatches READ swatches NOTIFY changed)
    Q_PROPERTY(QColor accent READ accent NOTIFY changed)
    Q_PROPERTY(bool light READ light NOTIFY changed)
    Q_PROPERTY(QString themeName READ themeName NOTIFY changed)
    Q_PROPERTY(QString family READ family NOTIFY changed)
    Q_PROPERTY(QString monoFamily READ monoFamily NOTIFY changed)
    /// Text size as a multiple of the normal (0.9–1.25).
    Q_PROPERTY(double textScale READ textScale NOTIFY changed)
    /// "compact", "comfortable" or "relaxed".
    Q_PROPERTY(QString density READ density NOTIFY changed)
    Q_PROPERTY(int radiusSmall READ radiusSmall NOTIFY changed)
    Q_PROPERTY(int radiusCard READ radiusCard NOTIFY changed)
    Q_PROPERTY(int radiusDialog READ radiusDialog NOTIFY changed)
    Q_PROPERTY(qint64 generation READ generation NOTIFY changed)
    /// The swatch chosen for Files in Settings (`amber`, …); empty: the theme's own.
    Q_PROPERTY(QString accentChoice READ accentChoice WRITE setAccentChoice NOTIFY changed)
    /// Whether Nyx Settings is installed to change the theme, text size and density.
    Q_PROPERTY(bool canOpenThemeSettings READ canOpenThemeSettings CONSTANT)
public:
    explicit SystemTheme(QObject *parent = nullptr);

    /// Makes `SystemTheme` the singleton of `import AscFiles.System`: one for each engine.
    static void registerType();

    /// `$XDG_RUNTIME_DIR/nyx/theme`, else the cache's fallback folder, as nyx-theme-core has it.
    static QString runtimeDir();
    static QString suitePath();

    /// A colour token (`surface.primary`), or `fallback` where the theme has none.
    Q_INVOKABLE QColor color(const QString &token, const QColor &fallback = QColor()) const;
    Q_INVOKABLE void reload();
    /// Opens Nyx Settings on its Appearance page.
    Q_INVOKABLE void openThemeSettings() const;

    QVariantMap colors() const { return m_colors; }
    QVariantMap swatches() const { return m_swatches; }
    QColor accent() const { return m_accent; }
    bool light() const { return m_light; }
    QString themeName() const { return m_themeName; }
    QString family() const { return m_family; }
    QString monoFamily() const { return m_monoFamily; }
    double textScale() const { return m_textScale; }
    QString density() const { return m_density; }
    int radiusSmall() const { return m_radiusSmall; }
    int radiusCard() const { return m_radiusCard; }
    int radiusDialog() const { return m_radiusDialog; }
    qint64 generation() const { return m_generation; }
    QString accentChoice() const { return m_accentChoice; }
    /// Keeps the choice in the suite's settings, as Nyx's own apps do (`[app_accents] files`),
    /// changing that one line only.
    void setAccentChoice(const QString &swatch);
    bool canOpenThemeSettings() const;
    /// `text` with `[app_accents] files` set to `swatch`, or taken out when it is empty.
    static QString withFilesAccent(const QString &text, const QString &swatch);

signals:
    void changed();

private:
    void loadFallback();
    void installedFonts();
    bool loadSnapshot();
    void loadSuite();
    void watch();

    QFileSystemWatcher m_watch;
    QVariantMap m_colors;
    QVariantMap m_swatches;
    QVariantMap m_accents;
    QColor m_accent;
    bool m_light = false;
    QString m_themeName;
    QString m_family = QStringLiteral("Inter");
    QString m_monoFamily = QStringLiteral("JetBrains Mono");
    double m_textScale = 1.0;
    QString m_density = QStringLiteral("comfortable");
    QString m_accentChoice;
    int m_radiusSmall = 7;
    int m_radiusCard = 11;
    int m_radiusDialog = 15;
    qint64 m_generation = -1;
};
