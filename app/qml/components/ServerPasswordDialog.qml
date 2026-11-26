// SPDX-License-Identifier: MIT
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

import AscFiles

// A server's request for a password, as GIO asks it: the user and domain where it needs them,
// connecting anonymously where it allows that, and how long to remember the password.
Popup {
    id: dialog

    property int asked: -1
    property string message
    property bool needsUser
    property bool needsDomain
    property bool anonymousAllowed
    property bool canRemember
    property string remember: "never"
    signal given(int asked, string user, string password, string domain, bool anonymous, string remember)
    signal cancelled(int asked)

    function ask(asked, message, user, domain, needsUser, needsDomain, anonymousAllowed, canRemember) {
        dialog.asked = asked
        dialog.message = message
        dialog.needsUser = needsUser
        dialog.needsDomain = needsDomain
        dialog.anonymousAllowed = anonymousAllowed
        dialog.canRemember = canRemember
        dialog.remember = "never"
        userField.text = user
        domainField.text = domain
        passwordField.text = ""
        anonymous.checked = false
        open()
        (needsUser && user.length === 0 ? userField : passwordField).forceActiveFocus()
    }
    function accept() {
        const asked = dialog.asked
        dialog.asked = -1
        dialog.close()
        dialog.given(asked, userField.text, passwordField.text, domainField.text, anonymous.checked, dialog.remember)
        passwordField.text = ""
    }
    onClosed: if (asked >= 0) { const was = asked; asked = -1; cancelled(was) }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: 440
    modal: true
    focus: true
    padding: 22
    closePolicy: Popup.CloseOnEscape
    Overlay.modal: Rectangle { color: Theme.scrim }
    background: Rectangle { radius: 14; color: Theme.raised; border.width: 1; border.color: Theme.panelEdge }

    component Field: TextField {
        Layout.fillWidth: true
        implicitHeight: 30
        leftPadding: 10
        color: Theme.text
        font.family: Theme.family
        font.pointSize: Theme.body
        placeholderTextColor: Theme.textMute
        enabled: !anonymous.checked
        opacity: enabled ? 1 : 0.5
        background: Rectangle { radius: Theme.radius; color: Theme.field; border.width: 1; border.color: parent.activeFocus ? Theme.selectionEdge : Theme.panelEdge }
        onAccepted: dialog.accept()
    }

    contentItem: ColumnLayout {
        spacing: 12
        Text {
            Layout.fillWidth: true
            text: dialog.message.split("\n")[0]
            wrapMode: Text.Wrap
            color: Theme.text
            font.family: Theme.family
            font.pointSize: Theme.title
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            visible: text.length > 0
            text: dialog.message.split("\n").slice(1).filter(line => line.trim().length > 0).join("\n")
            wrapMode: Text.Wrap
            color: Theme.textDim
            font.family: Theme.family
            font.pointSize: Theme.body
        }
        CheckBox { id: anonymous; visible: dialog.anonymousAllowed; text: qsTr("Connect anonymously") }
        Field { id: userField; visible: dialog.needsUser; placeholderText: qsTr("User") }
        Field { id: domainField; visible: dialog.needsDomain; placeholderText: qsTr("Domain") }
        Field { id: passwordField; objectName: "serverPassword"; placeholderText: qsTr("Password"); echoMode: TextInput.Password }
        Segmented {
            visible: dialog.canRemember && !anonymous.checked
            options: [{ label: qsTr("Forget"), value: "never" }, { label: qsTr("Until logout"), value: "session" }, { label: qsTr("Remember"), value: "always" }]
            value: dialog.remember
            onPicked: (value) => dialog.remember = value
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            Layout.topMargin: 6
            spacing: 8
            Button { text: qsTr("Cancel"); onClicked: dialog.close() }
            PrimaryButton { text: qsTr("Connect"); onClicked: dialog.accept() }
        }
    }
}
