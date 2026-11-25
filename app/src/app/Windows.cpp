// SPDX-License-Identifier: MIT
#include "Windows.h"

#include "backend/BackendClient.h"
#include "desktop/DesktopApps.h"
#include "models/Properties.h"
#include "models/SearchModel.h"
#include "models/TabModel.h"
#include "render/Preview.h"
#include "settings/AppSettings.h"

#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QQmlComponent>
#include <QQmlEngine>

Windows::Windows(QQmlEngine *engine, QVariantMap shared, QObject *parent)
    : QObject(parent), m_engine(engine), m_shared(std::move(shared)) {
    m_shared.insert(QStringLiteral("windows"), QVariant::fromValue(this));
    connect(qGuiApp, &QGuiApplication::focusWindowChanged, this, [this](QWindow *focused) {
        auto *window = qobject_cast<QQuickWindow *>(focused);
        if (window && m_windows.contains(window) && window != m_current) {
            m_current = window;
            emit currentChanged();
        }
    });
}

Windows::~Windows() {
    for (const QPointer<QQuickWindow> &window : std::as_const(m_closed)) {
        delete window.data();
    }
    qDeleteAll(m_windows);
}

QQuickWindow *Windows::open(const QStringList &asked) {
    // A page that isn't a folder (the Overview, Search) gives no folder: the window opens on the
    // start folder.
    QStringList folders = asked;
    folders.removeAll(QString());
    auto *backend = m_shared.value(QStringLiteral("backend")).value<BackendClient *>();
    // The window's own models live as long as it does: they are its children.
    auto *tabs = new TabModel(backend);
    auto *search = new SearchModel(backend);
    auto *preview = new Preview;
    auto *properties = new Properties(backend, m_shared.value(QStringLiteral("apps")).value<DesktopApps *>());
    QVariantMap initial = m_shared;
    initial.insert(QStringLiteral("tabs"), QVariant::fromValue(tabs));
    initial.insert(QStringLiteral("search"), QVariant::fromValue(search));
    initial.insert(QStringLiteral("preview"), QVariant::fromValue(preview));
    initial.insert(QStringLiteral("properties"), QVariant::fromValue(properties));

    QQmlComponent component(m_engine, QStringLiteral("AscFiles"), QStringLiteral("Main"));
    QObject *made = component.createWithInitialProperties(initial);
    auto *window = qobject_cast<QQuickWindow *>(made);
    if (!window) {
        qWarning().noquote() << component.errorString();
        delete made;
        delete tabs;
        delete search;
        delete preview;
        delete properties;
        emit failed();
        return nullptr;
    }
    QQmlEngine::setObjectOwnership(window, QQmlEngine::CppOwnership);
    for (QObject *own : {static_cast<QObject *>(tabs), static_cast<QObject *>(search), static_cast<QObject *>(preview),
                         static_cast<QObject *>(properties)}) {
        own->setParent(window);
    }

    const bool keepsSession = !m_sessionTaken;
    m_sessionTaken = true;
    tabs->setKeepsSession(keepsSession);
    if (folders.isEmpty() && !(keepsSession && tabs->restoreSession())) {
        tabs->openTab(AppSettings::shared().startFolderOrHome());
    }
    for (const QString &folder : folders) {
        tabs->openTab(QDir(folder).absolutePath());
    }
    if (!folders.isEmpty()) {
        tabs->setCurrentIndex(0);
    }

    // A closed window is only hidden by Qt Quick; it goes, with its models, once hidden.
    connect(window, &QWindow::visibleChanged, this, [this, window](bool visible) {
        if (!visible) {
            closed(window);
        }
    });
    m_windows.append(window);
    emit countChanged();
    if (!m_current) {
        m_current = window;
        emit currentChanged();
    }
    window->show();
    window->requestActivate();
    return window;
}

void Windows::closed(QQuickWindow *window) {
    if (!m_windows.removeOne(window)) {
        return;
    }
    if (m_current == window) {
        m_current = m_windows.isEmpty() ? nullptr : m_windows.last();
        emit currentChanged();
    }
    emit countChanged();
    m_closed.removeIf([](const QPointer<QQuickWindow> &gone) { return gone.isNull(); });
    m_closed.append(window);
    window->deleteLater();
}

void Windows::reveal(const QByteArray &folder, const QList<QByteArray> &items, bool properties) {
    QQuickWindow *window = open({QFile::decodeName(folder)});
    if (!window) {
        return;
    }
    QStringList paths;
    for (const QByteArray &item : items) {
        paths.append(QFile::decodeName(item));
    }
    QMetaObject::invokeMethod(window, "revealItems", Q_ARG(QVariant, paths), Q_ARG(QVariant, properties));
    window->requestActivate();
}
