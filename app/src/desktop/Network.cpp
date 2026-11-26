// SPDX-License-Identifier: MIT
#include "Network.h"

#include <QFile>
#include <QUrl>

// GIO names a struct member `signals`, which Qt defines.
#pragma push_macro("signals")
#undef signals
#include <gio/gio.h>
#pragma pop_macro("signals")

namespace {

void onAskPassword(GMountOperation *operation, const char *message, const char *user, const char *domain, GAskPasswordFlags flags, gpointer self) {
    static_cast<Network *>(self)->askPassword(operation, message, user, domain, int(flags));
}

void onAskQuestion(GMountOperation *operation, const char *message, const char **choices, gpointer self) {
    static_cast<Network *>(self)->askQuestion(operation, message, choices);
}

void onMountsChanged(GVolumeMonitor *, GMount *, gpointer self) {
    auto *network = static_cast<Network *>(self);
    QMetaObject::invokeMethod(network, [network] { network->refresh(); }, Qt::QueuedConnection);
}

/// What a mount is called and where gvfs shows it.
QVariantMap describe(GMount *mount) {
    GFile *root = g_mount_get_root(mount);
    char *uri = g_file_get_uri(root);
    char *path = g_file_get_path(root);
    char *name = g_mount_get_name(mount);
    QVariantMap described{{"name", QString::fromUtf8(name)}, {"uri", QString::fromUtf8(uri)}, {"path", path ? QFile::decodeName(path) : QString()}};
    g_free(uri);
    g_free(path);
    g_free(name);
    g_object_unref(root);
    return described;
}

} // namespace

Network::Network(QObject *parent) : QObject(parent) {
    m_monitor = g_volume_monitor_get();
    for (const char *signal : {"mount-added", "mount-removed", "mount-changed"}) {
        g_signal_connect(m_monitor, signal, G_CALLBACK(onMountsChanged), this);
    }
    refresh();
}

Network::~Network() {
    g_signal_handlers_disconnect_by_data(m_monitor, this);
    g_object_unref(m_monitor);
    for (GMountOperation *operation : std::as_const(m_asking)) {
        g_mount_operation_reply(operation, G_MOUNT_OPERATION_ABORTED);
        g_object_unref(operation);
    }
}

void Network::refresh() {
    QVariantList mounts;
    GList *all = g_volume_monitor_get_mounts(m_monitor);
    for (GList *item = all; item; item = item->next) {
        GMount *mount = G_MOUNT(item->data);
        GFile *root = g_mount_get_root(mount);
        // The network ones: their root is not a local file (it is reached through gvfs).
        if (!g_file_has_uri_scheme(root, "file")) {
            mounts.append(describe(mount));
        }
        g_object_unref(root);
    }
    g_list_free_full(all, g_object_unref);
    if (mounts != m_mounts) {
        m_mounts = mounts;
        emit changed();
    }
}

bool Network::isAddress(const QString &text) {
    const QUrl url(text.trimmed(), QUrl::StrictMode);
    // Somewhere to go: a host, or (for local schemes) a path.
    return text.contains(QLatin1String("://")) && url.isValid() && !url.scheme().isEmpty() && url.scheme() != QLatin1String("file") &&
           (!url.host().isEmpty() || url.path().length() > 1);
}

GMountOperation *Network::operation() {
    GMountOperation *operation = g_mount_operation_new();
    g_signal_connect(operation, "ask-password", G_CALLBACK(onAskPassword), this);
    g_signal_connect(operation, "ask-question", G_CALLBACK(onAskQuestion), this);
    return operation;
}

void Network::connectTo(const QString &address) {
    if (!isAddress(address)) {
        emit failed(tr("Enter an address such as smb://server/share"));
        return;
    }
    GFile *file = g_file_new_for_uri(address.trimmed().toUtf8().constData());
    GMountOperation *operation = this->operation();
    struct Context {
        Network *self;
        GMountOperation *operation;
    };
    g_file_mount_enclosing_volume(
        file, G_MOUNT_MOUNT_NONE, operation, nullptr,
        [](GObject *source, GAsyncResult *result, gpointer data) {
            auto *context = static_cast<Context *>(data);
            context->self->finishConnect(source, result);
            g_signal_handlers_disconnect_by_data(context->operation, context->self);
            g_object_unref(context->operation);
            delete context;
        },
        new Context{this, operation});
    g_object_unref(file);
}

void Network::finishConnect(void *source, void *result) {
    GFile *file = G_FILE(source);
    GError *error = nullptr;
    const bool mounted = g_file_mount_enclosing_volume_finish(file, G_ASYNC_RESULT(result), &error);
    if (!mounted && error && !g_error_matches(error, G_IO_ERROR, G_IO_ERROR_ALREADY_MOUNTED)) {
        // Cancelled by the user, or a prompt they closed: nothing to say.
        if (!g_error_matches(error, G_IO_ERROR, G_IO_ERROR_FAILED_HANDLED) && !g_error_matches(error, G_IO_ERROR, G_IO_ERROR_CANCELLED)) {
            emit failed(QString::fromUtf8(error->message));
        }
        g_error_free(error);
        return;
    }
    if (error) {
        g_error_free(error);
    }
    char *path = g_file_get_path(file);
    if (!path) {
        emit failed(tr("Connected, but the server can't be shown as a folder here (gvfs's FUSE folder is missing)."));
        return;
    }
    const QString folder = QFile::decodeName(path);
    g_free(path);
    refresh();
    emit opened(folder);
}

void Network::disconnectFrom(const QString &uri) {
    GFile *file = g_file_new_for_uri(uri.toUtf8().constData());
    GMount *mount = g_file_find_enclosing_mount(file, nullptr, nullptr);
    g_object_unref(file);
    if (!mount) {
        return;
    }
    GMountOperation *operation = this->operation();
    g_mount_unmount_with_operation(
        mount, G_MOUNT_UNMOUNT_NONE, operation, nullptr,
        [](GObject *source, GAsyncResult *result, gpointer data) {
            auto *self = static_cast<Network *>(data);
            GError *error = nullptr;
            if (!g_mount_unmount_with_operation_finish(G_MOUNT(source), result, &error) && error) {
                if (!g_error_matches(error, G_IO_ERROR, G_IO_ERROR_FAILED_HANDLED)) {
                    emit self->failed(QString::fromUtf8(error->message));
                }
                g_error_free(error);
            }
            self->refresh();
        },
        this);
    g_object_unref(operation);
    g_object_unref(mount);
}

int Network::ask(GMountOperation *operation) {
    const int asked = ++m_next;
    m_asking.insert(asked, G_MOUNT_OPERATION(g_object_ref(operation)));
    return asked;
}

void Network::askPassword(GMountOperation *operation, const char *message, const char *user, const char *domain, int flags) {
    const int asked = ask(operation);
    emit passwordWanted(asked, QString::fromUtf8(message), QString::fromUtf8(user), QString::fromUtf8(domain), flags & G_ASK_PASSWORD_NEED_USERNAME,
                        flags & G_ASK_PASSWORD_NEED_DOMAIN, flags & G_ASK_PASSWORD_ANONYMOUS_SUPPORTED, flags & G_ASK_PASSWORD_SAVING_SUPPORTED);
}

void Network::askQuestion(GMountOperation *operation, const char *message, const char **choices) {
    const int asked = ask(operation);
    QStringList list;
    for (const char **choice = choices; choice && *choice; ++choice) {
        list.append(QString::fromUtf8(*choice));
    }
    emit questionAsked(asked, QString::fromUtf8(message), list);
}

void Network::answerPassword(int asked, const QString &user, const QString &password, const QString &domain, bool anonymous,
                             const QString &remember) {
    GMountOperation *operation = m_asking.take(asked);
    if (!operation) {
        return;
    }
    g_mount_operation_set_anonymous(operation, anonymous);
    if (!anonymous) {
        g_mount_operation_set_username(operation, user.toUtf8().constData());
        g_mount_operation_set_password(operation, password.toUtf8().constData());
        g_mount_operation_set_domain(operation, domain.toUtf8().constData());
    }
    g_mount_operation_set_password_save(operation, remember == QLatin1String("always")    ? G_PASSWORD_SAVE_PERMANENTLY
                                                   : remember == QLatin1String("session") ? G_PASSWORD_SAVE_FOR_SESSION
                                                                                          : G_PASSWORD_SAVE_NEVER);
    g_mount_operation_reply(operation, G_MOUNT_OPERATION_HANDLED);
    g_object_unref(operation);
}

void Network::answerQuestion(int asked, int choice) {
    GMountOperation *operation = m_asking.take(asked);
    if (!operation) {
        return;
    }
    g_mount_operation_set_choice(operation, choice);
    g_mount_operation_reply(operation, G_MOUNT_OPERATION_HANDLED);
    g_object_unref(operation);
}

void Network::cancel(int asked) {
    GMountOperation *operation = m_asking.take(asked);
    if (!operation) {
        return;
    }
    g_mount_operation_reply(operation, G_MOUNT_OPERATION_ABORTED);
    g_object_unref(operation);
}
