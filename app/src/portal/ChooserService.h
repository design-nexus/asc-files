// SPDX-License-Identifier: MIT
#pragma once

#include "Chooser.h"

#include <QDBusArgument>
#include <QDBusContext>
#include <QDBusMessage>
#include <QDBusObjectPath>
#include <QObject>
#include <QPointer>
#include <QSize>
#include <QVariantMap>

Q_DECLARE_METATYPE(Portal::Rule)
Q_DECLARE_METATYPE(Portal::Filter)
Q_DECLARE_METATYPE(Portal::Choice)

namespace Portal {

/// A chosen value of a choice, as the portal answers it: `(ss)`.
struct ChoiceValue {
    QString id;
    QString value;
};

/// Registers the portal's types with QtDBus; once, before the service answers.
void registerTypes();

/// A request's options as the portal sends them (`a{sv}`, with filters, choices and paths as their
/// D-Bus types), read into `Options`; values already of these types are taken as they are.
Options optionsFrom(const QVariantMap &options);

/// One chooser request in progress, which its window shows and answers.
class ChooserJob : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString kind READ kind CONSTANT)
    Q_PROPERTY(QString title READ title CONSTANT)
    Q_PROPERTY(QString acceptLabel READ acceptLabel CONSTANT)
    Q_PROPERTY(bool directory READ directory CONSTANT)
    Q_PROPERTY(bool multiple READ multiple CONSTANT)
    Q_PROPERTY(QString folder READ folder CONSTANT)
    Q_PROPERTY(QString currentName READ currentName CONSTANT)
    /// SaveFiles: the names saved.
    Q_PROPERTY(QStringList names READ names CONSTANT)
    /// `[{label, globs}]`: each filter as the globs the folder lists by (MIME types as theirs).
    Q_PROPERTY(QVariantList filters READ filters CONSTANT)
    Q_PROPERTY(int currentFilter READ currentFilter CONSTANT)
    /// `[{id, label, options: [{id, label}], initial}]`; no options makes a check box.
    Q_PROPERTY(QVariantList choices READ choices CONSTANT)
    Q_PROPERTY(QSize sizeHint READ sizeHint CONSTANT)
public:
    ChooserJob(Request request, QObject *parent = nullptr);

    QString kind() const;
    QString title() const { return m_request.title; }
    QString acceptLabel() const { return m_request.acceptLabel; }
    bool directory() const { return m_request.directory; }
    bool multiple() const { return m_request.multiple; }
    QString folder() const;
    QString currentName() const;
    QStringList names() const;
    QVariantList filters() const;
    int currentFilter() const;
    QVariantList choices() const;
    const Request &request() const { return m_request; }
    /// The caller's window size, where the compositor told it; the chooser opens that size.
    QSize sizeHint() const { return m_sizeHint; }
    void setSizeHint(QSize size) { m_sizeHint = size; }

    /// Open: answers with `paths` (the selection; none in a folder chooser means `current`), or
    /// says why they won't do.
    Q_INVOKABLE QString acceptOpen(const QStringList &paths, const QString &current, const QVariantMap &choices, int filter);
    /// SaveFile and SaveFiles: what would be replaced (file names), or a refusal as `error`;
    /// `replace` confirms replacing them. Answers when nothing stands in the way.
    Q_INVOKABLE QVariantMap acceptSave(const QString &folder, const QString &name, const QVariantMap &choices, int filter, bool replace);
    /// The user closed the chooser.
    Q_INVOKABLE void cancel();

signals:
    /// Answered: `response` 0 chosen, 1 cancelled, 2 failed; `results` as the portal answers them.
    void finished(uint response, const QVariantMap &results);

private:
    void answer(const QList<QByteArray> &paths, const QVariantMap &choices, int filter);
    Request m_request;
    QSize m_sizeHint;
    bool m_done = false;
};

/// `org.freedesktop.impl.portal.FileChooser`: each request becomes a `ChooserJob` for a window to
/// show (`chooserWanted`), answered when it finishes; the caller may close it meanwhile.
class ChooserService : public QObject, protected QDBusContext {
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.freedesktop.impl.portal.FileChooser")
    Q_PROPERTY(uint version READ version)
public:
    explicit ChooserService(QObject *parent = nullptr);
    uint version() const { return FileChooserVersion; }
    RequestTracker &tracker() { return m_tracker; }

public slots:
    Q_SCRIPTABLE uint OpenFile(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const QString &title,
                               const QVariantMap &options, QVariantMap &results);
    Q_SCRIPTABLE uint SaveFile(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const QString &title,
                               const QVariantMap &options, QVariantMap &results);
    Q_SCRIPTABLE uint SaveFiles(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const QString &title,
                                const QVariantMap &options, QVariantMap &results);

signals:
    /// A window should show `job`; it is deleted once answered.
    void chooserWanted(Portal::ChooserJob *job);

private:
    uint start(const QDBusObjectPath &handle, const QString &appId, const QString &parentWindow, const Checked &checked);
    RequestTracker m_tracker;
};

} // namespace Portal

Q_DECLARE_METATYPE(Portal::ChoiceValue)
