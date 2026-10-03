import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

ColumnLayout {
    id: root

    property string cfg_File: ""
    property string cfg_Kind: "video"
    property string cfg_Fit: "cover"
    property int cfg_Volume: 0
    property bool cfg_Muted: true
    property bool cfg_Paused: false

    Label {
        text: "Experimental: playback runs inside Plasma. Only local media is accepted."
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
    RowLayout {
        Layout.fillWidth: true
        TextField {
            text: root.cfg_File
            placeholderText: "/absolute/path/to/media or file:/// URL"
            Layout.fillWidth: true
            onTextEdited: root.cfg_File = text
        }
        Button {
            text: "Choose file…"
            onClicked: picker.open()
        }
    }
    FileDialog {
        id: picker
        title: "Choose local wallpaper media"
        fileMode: FileDialog.OpenFile
        onAccepted: root.cfg_File = selectedFile.toString()
    }
    RowLayout {
        Label { text: "Media type" }
        ComboBox {
            model: ["image", "video"]
            currentIndex: root.cfg_Kind === "image" ? 0 : 1
            onActivated: root.cfg_Kind = currentText
        }
        Label { text: "Fit" }
        ComboBox {
            model: ["cover", "contain", "fill"]
            currentIndex: Math.max(0, model.indexOf(root.cfg_Fit))
            onActivated: root.cfg_Fit = currentText
        }
    }
    RowLayout {
        Label { text: "Volume" }
        SpinBox {
            from: 0
            to: 100
            value: root.cfg_Volume
            onValueModified: root.cfg_Volume = value
        }
        CheckBox {
            text: "Muted"
            checked: root.cfg_Muted
            onToggled: root.cfg_Muted = checked
        }
        CheckBox {
            text: "Paused"
            checked: root.cfg_Paused
            onToggled: root.cfg_Paused = checked
        }
    }
}
