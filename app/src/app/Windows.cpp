// SPDX-License-Identifier: MIT
#include "Windows.h"

#include "backend/BackendClient.h"
#include "models/SearchModel.h"
#include "models/TabModel.h"
#include "render/Preview.h"
#include "settings/AppSettings.h"

#include <QDir>
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
    QVariantMap properties = m_shared;
    properties.insert(QStringLiteral("tabs"), QVariant::fromValue(tabs));
    properties.insert(QStringLiteral("search"), QVariant::fromValue(search));
    properties.insert(QStringLiteral("preview"), QVariant::fromValue(preview));

    QQmlComponent component(m_engine, QStringLiteral("AscFiles"), QStringLiteral("Main"));
    QObject *made = component.createWithInitialProperties(properties);
    auto *window = qobject_cast<QQuickWindow *>(made);
    if (!window) {
        qWarning().noquote() << component.errorString();
        delete made;
        delete tabs;
        delete search;
        delete preview;
        emit failed();
        return nullptr;
    }
    QQmlEngine::setObjectOwnership(window, QQmlEngine::CppOwnership);
    for (QObject *own : {static_cast<QObject *>(tabs), static_cast<QObject *>(search), static_cast<QObject *>(preview)}) {
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
    window->deleteLater();
}
