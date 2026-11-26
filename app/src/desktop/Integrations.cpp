// SPDX-License-Identifier: MIT
#include "Integrations.h"

#include "../backend/BackendClient.h"
#include "../backend/Wire.h"

#include <QCoreApplication>
#include <QFile>
#include <QJsonObject>

namespace {

/// Files' own program: what the desktop starts for it.
QJsonValue executable() { return Wire::encode(QFile::encodeName(QCoreApplication::applicationFilePath())); }

} // namespace

Integrations::Integrations(BackendClient *backend, QObject *parent) : QObject(parent), m_backend(backend) {}

void Integrations::refresh() {
    m_backend->call(QStringLiteral("integrations.status"), {{"executable", executable()}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        if (!error.isNull()) {
                            return;
                        }
                        const QJsonObject status = result.toObject();
                        const QJsonObject chooser = status.value(QStringLiteral("chooser")).toObject();
                        const QJsonObject fileManager = status.value(QStringLiteral("fileManager")).toObject();
                        m_chooser = chooser.value(QStringLiteral("preferred")).toBool() && chooser.value(QStringLiteral("activatable")).toBool();
                        m_fileManager = fileManager.value(QStringLiteral("default")).toBool();
                        emit changed();
                    });
}

void Integrations::setChooser(bool enable) { change(QStringLiteral("integrations.chooser"), enable); }

void Integrations::setFileManager(bool enable) { change(QStringLiteral("integrations.fileManager"), enable); }

void Integrations::change(const QString &method, bool enable) {
    m_busy = true;
    emit changed();
    m_backend->call(method, {{"executable", executable()}, {"enable", enable}}, this,
                    [this](const QJsonValue &result, const BackendError &error) {
                        m_busy = false;
                        if (!error.isNull()) {
                            emit notice(error.message);
                        } else if (result.toObject().value(QStringLiteral("keptEdits")).toBool()) {
                            emit notice(tr("Your own changes to the portal configuration were kept; only Files was taken out."));
                        }
                        refresh();
                    });
}
