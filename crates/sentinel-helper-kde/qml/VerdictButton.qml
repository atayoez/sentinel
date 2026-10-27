// SPDX-FileCopyrightText: 2025 Atay Özcan <atay@oezcan.me>
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Preserve native button styling while making keyboard selection explicit.

import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

QQC2.Button {
    id: button

    // Overlay the focus ring without replacing the native background.
    Rectangle {
        anchors.fill: parent
        anchors.margins: -2
        radius: Kirigami.Units.cornerRadius + 2
        color: "transparent"
        border.width: 3
        border.color: Kirigami.Theme.highlightColor
        visible: button.activeFocus
        z: 1
    }
}
