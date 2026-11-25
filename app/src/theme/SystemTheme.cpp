// SPDX-License-Identifier: MIT
#include "SystemTheme.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QSaveFile>
#include <QtQml/qqml.h>
#include <QStandardPaths>
#include <QTimer>

namespace {

/// Nyx Dark, the theme used until a snapshot can be read (nyx-theme-core's built-in default).
const char *const FallbackColors[][2] = {
    {"accent.hover", "#5195e9"},       {"accent.pressed", "#3371bd"},     {"accent.primary", "#3d8ae8"},
    {"accent.subtle", "#152234"},      {"border.default", "#2b2d37"},     {"border.focus", "#3d8ae8"},
    {"border.strong", "#606269"},      {"border.subtle", "#1d1e26"},      {"scrim.color", "#0a0a0e"},
    {"selection.background", "#1b3352"}, {"selection.foreground", "#e7e8ee"}, {"shadow.color", "#060609"},
    {"state.hover", "#10151f"},        {"state.selected", "#1f2028"},     {"status.error", "#f2655f"},
    {"status.info", "#3d8ae8"},        {"status.success", "#59d45b"},     {"status.warning", "#eeb43f"},
    {"surface.alternate", "#1d1e26"},  {"surface.elevated", "#1f2028"},   {"surface.overlay", "#1d1e26"},
    {"surface.primary", "#0d0e12"},    {"surface.secondary", "#121318"},  {"surface.tertiary", "#191a21"},
    {"text.disabled", "#5e6174"},      {"text.onAccent", "#0d0e12"},      {"text.primary", "#e7e8ee"},
    {"text.secondary", "#a2a5ba"},     {"text.tertiary", "#8a8ea8"},
};
const char *const FallbackSwatches[][2] = {
    {"amber", "#eeb43f"}, {"blue", "#3d8ae8"}, {"coral", "#f5875a"}, {"graphite", "#a2a5ba"}, {"green", "#59d45b"},
    {"pink", "#c95fc6"},  {"red", "#f2655f"},  {"teal", "#3cc4c9"},  {"violet", "#8b6cf0"},
};

/// The value of `key = value` in a TOML table body (`section` empty: the top level), unquoted.
/// Only what Files reads from the suite file: plain keys with strings or numbers.
QString tomlValue(const QString &text, const QString &section, const QString &key) {
    QString current;
    for (QString line : text.split(u'\n')) {
        line = line.trimmed();
        if (line.startsWith(u'#') || line.isEmpty()) {
            continue;
        }
        if (line.startsWith(u'[')) {
            current = line.mid(1, line.indexOf(u']') - 1).trimmed();
            continue;
        }
        const qsizetype equals = line.indexOf(u'=');
        if (current != section || equals < 0 || line.left(equals).trimmed() != key) {
            continue;
        }
        QString value = line.mid(equals + 1).trimmed();
        const qsizetype comment = value.startsWith(u'"') ? -1 : value.indexOf(u'#');
        if (comment >= 0) {
            value = value.left(comment).trimmed();
        }
        if (value.startsWith(u'"') && value.endsWith(u'"') && value.size() >= 2) {
            value = value.mid(1, value.size() - 2);
        }
        return value;
    }
    return {};
}

} // namespace

SystemTheme::SystemTheme(QObject *parent) : QObject(parent) {
    connect(&m_watch, &QFileSystemWatcher::directoryChanged, this, &SystemTheme::reload);
    connect(&m_watch, &QFileSystemWatcher::fileChanged, this, &SystemTheme::reload);
    reload();
}

void SystemTheme::registerType() {
    qmlRegisterSingletonType<SystemTheme>("AscFiles.System", 1, 0, "SystemTheme",
                                          [](QQmlEngine *, QJSEngine *) { return new SystemTheme; });
}

QString SystemTheme::runtimeDir() {
    const QString runtime = qEnvironmentVariable("XDG_RUNTIME_DIR");
    if (!runtime.isEmpty() && QDir::isAbsolutePath(runtime)) {
        return runtime + QStringLiteral("/nyx/theme");
    }
    return QStandardPaths::writableLocation(QStandardPaths::GenericCacheLocation) + QStringLiteral("/nyx/theme-runtime");
}

QString SystemTheme::suitePath() {
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QStringLiteral("/nyx/theme.toml");
}

QColor SystemTheme::color(const QString &token, const QColor &fallback) const {
    const QVariant value = m_colors.value(token);
    return value.isValid() ? QColor(value.toString()) : fallback;
}

void SystemTheme::loadFallback() {
    m_colors.clear();
    for (const auto &[token, value] : FallbackColors) {
        m_colors.insert(QString::fromLatin1(token), QString::fromLatin1(value));
    }
    m_swatches.clear();
    for (const auto &[name, value] : FallbackSwatches) {
        m_swatches.insert(QString::fromLatin1(name), QString::fromLatin1(value));
    }
    m_accents.clear();
    m_light = false;
    m_themeName = QStringLiteral("Nyx Dark");
}

bool SystemTheme::loadSnapshot() {
    QFile file(runtimeDir() + QStringLiteral("/system.json"));
    if (!file.open(QIODevice::ReadOnly)) {
        return false;
    }
    const QJsonObject snapshot = QJsonDocument::fromJson(file.readAll()).object();
    const QJsonObject tokens = snapshot.value(QStringLiteral("tokens")).toObject();
    const QJsonObject colors = tokens.value(QStringLiteral("colors")).toObject();
    if (snapshot.value(QStringLiteral("format")).toInt() != 1 || colors.isEmpty()) {
        return false;
    }
    // Tokens the snapshot leaves out keep Nyx Dark's, so every one has a value.
    loadFallback();
    for (auto it = colors.begin(); it != colors.end(); ++it) {
        m_colors.insert(it.key(), it.value().toString());
    }
    const QJsonObject swatches = snapshot.value(QStringLiteral("swatches")).toObject();
    if (!swatches.isEmpty()) {
        m_swatches = swatches.toVariantMap();
    }
    m_accents = snapshot.value(QStringLiteral("accents")).toObject().toVariantMap();
    const QJsonObject theme = snapshot.value(QStringLiteral("theme")).toObject();
    m_light = theme.value(QStringLiteral("appearance")).toString() == QLatin1String("light");
    m_themeName = theme.value(QStringLiteral("name")).toString();
    m_generation = snapshot.value(QStringLiteral("generation")).toInteger();
    const QJsonObject typography = tokens.value(QStringLiteral("typography")).toObject();
    const QString ui = typography.value(QStringLiteral("ui")).toString().section(u',', 0, 0).trimmed();
    const QString mono = typography.value(QStringLiteral("mono")).toString().section(u',', 0, 0).trimmed();
    m_family = ui.isEmpty() ? QStringLiteral("Inter") : ui;
    m_monoFamily = mono.isEmpty() ? QStringLiteral("JetBrains Mono") : mono;
    const QJsonObject geometry = tokens.value(QStringLiteral("geometry")).toObject();
    m_radiusSmall = geometry.value(QStringLiteral("radius_small")).toInt(7);
    m_radiusCard = geometry.value(QStringLiteral("radius_card")).toInt(11);
    m_radiusDialog = geometry.value(QStringLiteral("radius_dialog")).toInt(15);
    return true;
}

void SystemTheme::loadSuite() {
    QFile file(suitePath());
    const QString text = file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString();
    bool ok = false;
    const double scale = tomlValue(text, {}, QStringLiteral("text_scale")).toDouble(&ok);
    m_textScale = ok ? qBound(0.9, scale, 1.25) : 1.0;
    const QString density = tomlValue(text, {}, QStringLiteral("density"));
    m_density = density == QLatin1String("compact") || density == QLatin1String("relaxed") ? density : QStringLiteral("comfortable");
    // The accent chosen for Files in Settings, by swatch name; Nyx's apps read it under their id.
    m_accentChoice = tomlValue(text, QStringLiteral("app_accents"), QStringLiteral("files"));
}

QString SystemTheme::withFilesAccent(const QString &text, const QString &swatch) {
    QStringList lines = text.split(u'\n');
    if (!lines.isEmpty() && lines.last().isEmpty()) {
        lines.removeLast();
    }
    const QString entry = QStringLiteral("files = \"%1\"").arg(swatch);
    qsizetype table = -1;
    for (qsizetype i = 0; i < lines.size(); ++i) {
        const QString line = lines.at(i).trimmed();
        if (line.startsWith(u'[')) {
            if (table >= 0) {
                break; // Past the table: no files key in it.
            }
            if (line.mid(1, line.indexOf(u']') - 1).trimmed() == QLatin1String("app_accents")) {
                table = i;
            }
            continue;
        }
        const qsizetype equals = line.indexOf(u'=');
        if (table >= 0 && equals > 0 && line.left(equals).trimmed() == QLatin1String("files")) {
            if (swatch.isEmpty()) {
                lines.removeAt(i);
            } else {
                lines[i] = entry;
            }
            return lines.join(u'\n') + u'\n';
        }
    }
    if (swatch.isEmpty()) {
        return text;
    }
    if (table >= 0) {
        lines.insert(table + 1, entry);
    } else {
        if (!lines.isEmpty() && !lines.last().trimmed().isEmpty()) {
            lines.append(QString());
        }
        lines.append(QStringLiteral("[app_accents]"));
        lines.append(entry);
    }
    return lines.join(u'\n') + u'\n';
}

void SystemTheme::setAccentChoice(const QString &swatch) {
    if (swatch == m_accentChoice || (!swatch.isEmpty() && !m_swatches.contains(swatch))) {
        return;
    }
    QFile current(suitePath());
    const QString text = current.open(QIODevice::ReadOnly) ? QString::fromUtf8(current.readAll()) : QString();
    current.close();
    QDir().mkpath(QFileInfo(suitePath()).path());
    QSaveFile file(suitePath());
    if (!file.open(QIODevice::WriteOnly)) {
        return;
    }
    file.write(withFilesAccent(text, swatch).toUtf8());
    if (file.commit()) {
        reload();
    }
}

bool SystemTheme::canOpenThemeSettings() const { return !QStandardPaths::findExecutable(QStringLiteral("nyx-settings")).isEmpty(); }

void SystemTheme::openThemeSettings() const {
    QProcess::startDetached(QStringLiteral("nyx-settings"), {QStringLiteral("--section"), QStringLiteral("appearance")});
}

void SystemTheme::reload() {
    if (!loadSnapshot()) {
        loadFallback();
    }
    loadSuite();
    const QVariant chosen = m_swatches.value(m_accentChoice);
    const QVariant themed = m_accents.value(QStringLiteral("files"));
    m_accent = QColor(chosen.isValid() ? chosen.toString() : themed.isValid() ? themed.toString() : m_colors.value(QStringLiteral("accent.primary")).toString());
    watch();
    emit changed();
}

void SystemTheme::watch() {
    // The snapshot is replaced by renaming a new file over it: the folder changes, and the
    // file watched before is gone. Both folders are watched again after each change.
    const QStringList watched = m_watch.files() + m_watch.directories();
    if (!watched.isEmpty()) {
        m_watch.removePaths(watched);
    }
    for (const QString &path : {runtimeDir(), runtimeDir() + QStringLiteral("/system.json"), QFileInfo(suitePath()).path(), suitePath()}) {
        if (QFileInfo::exists(path)) {
            m_watch.addPath(path);
        }
    }
}
