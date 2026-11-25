// SPDX-License-Identifier: MIT
#include "Customizations.h"

#include <QColor>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QQmlEngine>
#include <QSaveFile>
#include <QStandardPaths>
#include <QtQml/qqml.h>

namespace {

struct Choice {
    const char *name;
    const char *label;
    const char *value;
};

// Legacy's colours.
const Choice Colors[] = {
    {"red", QT_TRANSLATE_NOOP("Customizations", "Red"), "#e5484d"},
    {"orange", QT_TRANSLATE_NOOP("Customizations", "Orange"), "#f76b15"},
    {"yellow", QT_TRANSLATE_NOOP("Customizations", "Yellow"), "#e5a50a"},
    {"green", QT_TRANSLATE_NOOP("Customizations", "Green"), "#30a46c"},
    {"blue", QT_TRANSLATE_NOOP("Customizations", "Blue"), "#0090ff"},
    {"purple", QT_TRANSLATE_NOOP("Customizations", "Purple"), "#8e4ec6"},
    {"gray", QT_TRANSLATE_NOOP("Customizations", "Gray"), "#8b8d98"},
};

// Legacy's sixteen icons, drawn in Files' line style (16×16).
const Choice Icons[] = {
    {"documents", QT_TRANSLATE_NOOP("Customizations", "Documents"), "M4 2h5l3 3v9H4zM9 2v3h3M6 8h4M6 10.5h4"},
    {"downloads", QT_TRANSLATE_NOOP("Customizations", "Downloads"), "M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10"},
    {"code", QT_TRANSLATE_NOOP("Customizations", "Code"), "M5.5 5 2.5 8l3 3M10.5 5l3 3-3 3M9 3.5l-2 9"},
    {"archive", QT_TRANSLATE_NOOP("Customizations", "Archive"), "M2.5 3.5h11v3h-11zM3.5 6.5v6h9v-6M6.5 9h3"},
    {"pictures", QT_TRANSLATE_NOOP("Customizations", "Pictures"),
     "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM2.5 12l4-4 3 3 2-2 2.5 2.5"},
    {"videos", QT_TRANSLATE_NOOP("Customizations", "Videos"),
     "M3.5 4h6A1.5 1.5 0 0 1 11 5.5v5A1.5 1.5 0 0 1 9.5 12h-6A1.5 1.5 0 0 1 2 10.5v-5A1.5 1.5 0 0 1 3.5 4zM11 7l3-2v6l-3-2"},
    {"terminal", QT_TRANSLATE_NOOP("Customizations", "Terminal"),
     "M3 3h10a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM4.5 6.5 6.5 8l-2 1.5M8 10h3"},
    {"home", QT_TRANSLATE_NOOP("Customizations", "Home"), "M2.5 7.5 8 3l5.5 4.5V13h-11zM6.5 13v-3h3v3"},
    {"storage", QT_TRANSLATE_NOOP("Customizations", "Storage"),
     "M3.5 4h9A1.5 1.5 0 0 1 14 5.5v5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 10.5v-5A1.5 1.5 0 0 1 3.5 4zM2 8h12M4.5 10h.01"},
    {"network", QT_TRANSLATE_NOOP("Customizations", "Network"),
     "M8 2a6 6 0 1 1 0 12A6 6 0 0 1 8 2zM2 8h12M8 2c1.7 1.7 2.5 3.7 2.5 6S9.7 12.3 8 14M8 2C6.3 3.7 5.5 5.7 5.5 8S6.3 12.3 8 14"},
    {"computer", QT_TRANSLATE_NOOP("Customizations", "Computer"),
     "M3 3h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM6 13.5h4M8 11v2.5"},
    {"private", QT_TRANSLATE_NOOP("Customizations", "Private"),
     "M4.5 7.5h7a1 1 0 0 1 1 1V13a1 1 0 0 1-1 1h-7a1 1 0 0 1-1-1V8.5a1 1 0 0 1 1-1zM5.5 7.5V5.5a2.5 2.5 0 0 1 5 0v2M8 10v1.5"},
    {"pinned", QT_TRANSLATE_NOOP("Customizations", "Pinned"), "M9.5 2.5 13.5 6.5 11 7.5 8.5 10l-.5 3-5-5 3-.5L8.5 5zM5.5 10.5 2.5 13.5"},
    {"media", QT_TRANSLATE_NOOP("Customizations", "Media"), "M5 3.5v9l7-4.5z"},
    {"settings", QT_TRANSLATE_NOOP("Customizations", "Settings"),
     "M8 5.5a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"},
    {"tasks", QT_TRANSLATE_NOOP("Customizations", "Tasks"), "M7 4h6.5M7 8h6.5M7 12h6.5M2.5 4l1 1 1.5-2M2.5 8l1 1 1.5-2M2.5 12l1 1 1.5-2"},
};

const Choice *find(const Choice *begin, const Choice *end, const QString &name) {
    for (const Choice *choice = begin; choice != end; ++choice) {
        if (name == QLatin1String(choice->name)) {
            return choice;
        }
    }
    return nullptr;
}

} // namespace

Customizations &Customizations::shared() {
    static Customizations customizations;
    return customizations;
}

void Customizations::registerType() {
    qmlRegisterSingletonType<Customizations>("AscFiles.System", 1, 0, "Customizations", [](QQmlEngine *, QJSEngine *) {
        Customizations *customizations = &shared();
        // Shared by every engine; none owns it.
        QQmlEngine::setObjectOwnership(customizations, QQmlEngine::CppOwnership);
        return customizations;
    });
}

QString Customizations::filePath() {
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + QStringLiteral("/asc-files/customized.json");
}

Customizations::Customizations() {
    QFile file(filePath());
    if (!file.open(QIODevice::ReadOnly)) {
        return;
    }
    const QJsonObject saved = QJsonDocument::fromJson(file.readAll()).object();
    const QJsonObject colors = saved.value(QStringLiteral("colors")).toObject();
    for (auto it = colors.begin(); it != colors.end(); ++it) {
        m_colors.insert(it.key(), it.value().toString());
    }
    const QJsonObject icons = saved.value(QStringLiteral("icons")).toObject();
    for (auto it = icons.begin(); it != icons.end(); ++it) {
        m_icons.insert(it.key(), it.value().toString());
    }
}

QVariantList Customizations::colors() const {
    QVariantList list;
    for (const Choice &choice : Colors) {
        list.append(QVariantMap{{QStringLiteral("name"), QLatin1String(choice.name)},
                                {QStringLiteral("label"), tr(choice.label)},
                                {QStringLiteral("hex"), QLatin1String(choice.value)}});
    }
    return list;
}

QVariantList Customizations::icons() const {
    QVariantList list;
    for (const Choice &choice : Icons) {
        list.append(QVariantMap{{QStringLiteral("name"), QLatin1String(choice.name)},
                                {QStringLiteral("label"), tr(choice.label)},
                                {QStringLiteral("path"), QLatin1String(choice.value)}});
    }
    return list;
}

QString Customizations::colorOf(const QString &path) const {
    const QString color = m_colors.value(path);
    if (color.isEmpty()) {
        return {};
    }
    if (const Choice *preset = find(std::begin(Colors), std::end(Colors), color)) {
        return QLatin1String(preset->value);
    }
    return QColor::isValidColorName(color) && color.startsWith(u'#') ? color : QString();
}

QString Customizations::iconOf(const QString &path) const {
    const QString icon = m_icons.value(path);
    if (icon.startsWith(QLatin1String("emoji:"))) {
        return icon;
    }
    const Choice *choice = find(std::begin(Icons), std::end(Icons), icon);
    return choice ? QLatin1String(choice->value) : QString();
}

void Customizations::setColor(const QStringList &paths, const QString &color) {
    const bool valid = color.isEmpty() || find(std::begin(Colors), std::end(Colors), color) ||
                       (color.startsWith(u'#') && color.size() == 7 && QColor::isValidColorName(color));
    if (!valid || paths.isEmpty()) {
        return;
    }
    for (const QString &path : paths) {
        if (color.isEmpty()) m_colors.remove(path);
        else m_colors.insert(path, color.toLower());
    }
    save();
    emit changed(paths);
}

void Customizations::setIcon(const QStringList &paths, const QString &icon) {
    const bool valid = icon.isEmpty() || find(std::begin(Icons), std::end(Icons), icon) ||
                       (icon.startsWith(QLatin1String("emoji:")) && icon.size() > 6 && icon.size() <= 6 + 16);
    if (!valid || paths.isEmpty()) {
        return;
    }
    for (const QString &path : paths) {
        if (icon.isEmpty()) m_icons.remove(path);
        else m_icons.insert(path, icon);
    }
    save();
    emit changed(paths);
}

void Customizations::reset(const QStringList &paths) {
    for (const QString &path : paths) {
        m_colors.remove(path);
        m_icons.remove(path);
    }
    save();
    emit changed(paths);
}

void Customizations::save() const {
    QJsonObject colors;
    for (auto it = m_colors.cbegin(); it != m_colors.cend(); ++it) {
        colors.insert(it.key(), it.value());
    }
    QJsonObject icons;
    for (auto it = m_icons.cbegin(); it != m_icons.cend(); ++it) {
        icons.insert(it.key(), it.value());
    }
    QDir().mkpath(QFileInfo(filePath()).path());
    QSaveFile file(filePath());
    if (file.open(QIODevice::WriteOnly)) {
        file.write(QJsonDocument(QJsonObject{{"colors", colors}, {"icons", icons}}).toJson(QJsonDocument::Compact));
        file.commit();
    }
}
