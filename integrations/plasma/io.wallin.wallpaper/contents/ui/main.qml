import QtQuick
import QtMultimedia
import org.kde.plasma.plasmoid

WallpaperItem {
    id: root

    readonly property string kind: root.configuration.Kind
    readonly property string fit: root.configuration.Fit
    readonly property bool paused: root.configuration.Paused
    readonly property url localSource: {
        const path = String(root.configuration.File);
        if (path.startsWith("file:///"))
            return path;
        if (path.startsWith("/"))
            return "file://" + path.split("/").map(encodeURIComponent).join("/");
        return "";
    }
    readonly property string playbackError: {
        if (!localSource.toString())
            return "Choose a local image or video in wallpaper settings.";
        if (kind === "image" && image.status === Image.Error)
            return "The selected image could not be loaded.";
        if (kind === "video" && player.error !== MediaPlayer.NoError)
            return player.errorString;
        return "";
    }

    loading: kind === "image" ? image.status === Image.Loading
                             : player.mediaStatus === MediaPlayer.LoadingMedia
    onPausedChanged: syncPlayback()

    function syncPlayback() {
        if (kind !== "video" || !localSource.toString()) {
            player.stop();
        } else if (paused) {
            player.pause();
        } else {
            player.play();
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "black"
    }

    Image {
        id: image
        anchors.fill: parent
        source: root.kind === "image" ? root.localSource : ""
        visible: root.kind === "image"
        asynchronous: true
        fillMode: root.fit === "fill" ? Image.Stretch
                  : root.fit === "contain" ? Image.PreserveAspectFit : Image.PreserveAspectCrop
    }

    MediaPlayer {
        id: player
        source: root.kind === "video" ? root.localSource : ""
        loops: MediaPlayer.Infinite
        videoOutput: video
        audioOutput: AudioOutput {
            volume: Math.max(0, Math.min(100, root.configuration.Volume)) / 100
            muted: root.configuration.Muted
        }
        onSourceChanged: Qt.callLater(root.syncPlayback)
        onMediaStatusChanged: {
            if (mediaStatus === MediaPlayer.LoadedMedia)
                root.syncPlayback();
        }
        onErrorOccurred: (error, errorString) => console.error("Wall-in media:", errorString)
    }

    VideoOutput {
        id: video
        anchors.fill: parent
        visible: root.kind === "video"
        fillMode: root.fit === "fill" ? VideoOutput.Stretch
                  : root.fit === "contain" ? VideoOutput.PreserveAspectFit : VideoOutput.PreserveAspectCrop
    }

    Text {
        anchors.centerIn: parent
        width: Math.max(0, parent.width - 80)
        text: root.playbackError
        visible: text.length > 0
        color: "white"
        wrapMode: Text.Wrap
        horizontalAlignment: Text.AlignHCenter
    }

    Component.onCompleted: syncPlayback()
    Component.onDestruction: player.stop()
}
