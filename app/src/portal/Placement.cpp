// SPDX-License-Identifier: MIT
#include "Placement.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <poll.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

namespace Portal::Placement {

using Clock = std::chrono::steady_clock;

std::optional<QByteArray> socketPath(const QByteArray &runtime, const QString &signature) {
    const bool plain = !signature.isEmpty() && std::all_of(signature.begin(), signature.end(), [](QChar c) {
        return c.unicode() < 128 && (c.isLetterOrNumber() || c == u'_' || c == u'-');
    });
    if (!runtime.startsWith('/') || !plain) {
        return std::nullopt;
    }
    return runtime + "/hypr/" + signature.toLatin1() + "/.socket.sock";
}

std::optional<QByteArray> hyprlandSocket() {
    const QByteArray runtime = qgetenv("XDG_RUNTIME_DIR");
    const QString signature = qEnvironmentVariable("HYPRLAND_INSTANCE_SIGNATURE");
    if (runtime.isEmpty()) {
        return std::nullopt;
    }
    return socketPath(runtime, signature);
}

QList<QByteArray> centeringCommands() {
    const QByteArray klass = QByteArray("^") + QByteArray(ChooserClass).replace(".", "[.]") + "$";
    const QByteArray match = "keyword windowrule[asc-files-chooser-center]:match:class " + klass;
    return {"/eval hl.window_rule({name='asc-files-chooser-center',match={class='" + klass + "'},center=true})", "/" + match,
            "[[BATCH]] " + match + ";keyword windowrule[asc-files-chooser-center]:center on"};
}

std::optional<QByteArray> exchange(const QByteArray &socket, const QByteArray &command, qint64 maxReply,
                                   std::chrono::milliseconds timeout) {
    const auto deadline = Clock::now() + timeout;
    const auto left = [&] {
        return int(std::max<qint64>(0, std::chrono::duration_cast<std::chrono::milliseconds>(deadline - Clock::now()).count()));
    };
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    if (socket.size() >= qsizetype(sizeof(address.sun_path))) {
        return std::nullopt;
    }
    memcpy(address.sun_path, socket.constData(), socket.size());
    const int fd = ::socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd < 0) {
        return std::nullopt;
    }
    std::optional<QByteArray> reply;
    // One deadline for everything: connect, send, and the reply until the compositor closes.
    auto wait = [&](short events) {
        pollfd one{fd, events, 0};
        return ::poll(&one, 1, left()) == 1 && !(one.revents & (POLLERR | POLLNVAL));
    };
    if (::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0 || (errno == EINPROGRESS && wait(POLLOUT))) {
        qsizetype sent = 0;
        while (sent < command.size() && wait(POLLOUT)) {
            const ssize_t now = ::send(fd, command.constData() + sent, size_t(command.size() - sent), MSG_NOSIGNAL);
            if (now <= 0) {
                break;
            }
            sent += now;
        }
        if (sent == command.size()) {
            QByteArray read;
            char buffer[4096];
            while (read.size() <= maxReply && wait(POLLIN)) {
                const ssize_t now = ::recv(fd, buffer, sizeof(buffer), 0);
                if (now == 0) {
                    reply = read;
                    break;
                }
                if (now < 0) {
                    break;
                }
                read.append(buffer, now);
            }
            if (reply && reply->size() > maxReply) {
                reply.reset();
            }
        }
    }
    ::close(fd);
    return reply;
}

bool installCenteringRule(const QByteArray &socket, std::chrono::milliseconds timeout) {
    const auto deadline = Clock::now() + timeout;
    const auto left = [&] { return std::chrono::duration_cast<std::chrono::milliseconds>(deadline - Clock::now()); };
    const QList<QByteArray> commands = centeringCommands();
    const auto first = exchange(socket, commands[0], MaxCommandReplyBytes, left());
    if (!first) {
        return false;
    }
    const QByteArray answer = first->trimmed();
    if (answer == "ok") {
        return true;
    }
    if (answer != "unknown request" && answer != "eval is only supported with the lua config manager") {
        return false;
    }
    if (exchange(socket, commands[1], MaxCommandReplyBytes, left()) != QByteArray("ok")) {
        return false;
    }
    const auto batch = exchange(socket, commands[2], MaxCommandReplyBytes, left());
    if (!batch) {
        return false;
    }
    QList<QByteArray> parts;
    for (const QByteArray &part : batch->split('\n')) {
        if (!part.trimmed().isEmpty()) {
            parts.append(part.trimmed());
        }
    }
    return parts == QList<QByteArray>{"ok", "ok"};
}

std::optional<QSize> applicationWindowSize(const QByteArray &reply, const QString &appId) {
    if (appId.isEmpty()) {
        return std::nullopt;
    }
    const QJsonDocument document = QJsonDocument::fromJson(reply);
    if (!document.isArray()) {
        return std::nullopt;
    }
    std::optional<QJsonObject> found;
    for (const QJsonValue &value : document.array()) {
        const QJsonObject window = value.toObject();
        const bool matches = window.value("class").toString().compare(appId, Qt::CaseInsensitive) == 0 ||
                             window.value("initialClass").toString().compare(appId, Qt::CaseInsensitive) == 0;
        if (!matches) {
            continue;
        }
        if (found) {
            return std::nullopt;
        }
        found = window;
    }
    if (!found) {
        return std::nullopt;
    }
    const QJsonArray size = found->value("size").toArray();
    const QJsonValue mapped = found->value("mapped");
    const QJsonValue hidden = found->value("hidden");
    if (!mapped.isBool() || !hidden.isBool() || !mapped.toBool() || hidden.toBool() || size.size() != 2 || size[0].toInt() <= 0 ||
        size[1].toInt() <= 0) {
        return std::nullopt;
    }
    return QSize(size[0].toInt(), size[1].toInt());
}

std::optional<QSize> querySize(const QByteArray &socket, const QString &appId, std::chrono::milliseconds timeout) {
    const auto reply = exchange(socket, "j/clients", MaxReplyBytes, timeout);
    return reply ? applicationWindowSize(*reply, appId) : std::nullopt;
}

std::optional<QSize> parentSizeHint(const QString &appId, const QString &parentWindow) {
    // xdg-foreign exports transiency, not geometry: a hint only for a Wayland caller.
    if (!parentWindow.startsWith(QLatin1String("wayland:")) || appId.isEmpty()) {
        return std::nullopt;
    }
    const auto socket = hyprlandSocket();
    return socket ? querySize(*socket, appId, QueryTimeout) : std::nullopt;
}

} // namespace Portal::Placement
