view HorseTinder {
    state decision: str = ""
    derived actionOffset: i64 = (windowWidth - 320) / 2
    grid columns: 1fr 80 24 80 1fr
    grid rows: 46 1fr 54 34 72 auto auto
    grid gap: 8
    grid padding: 16
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "♥︎  HORSE TINDER"
        color: "#FF5A73"
        size: 18
        bold: true
        wrap: false
        letterSpacing: 1
        alignY: "center"

    Text nearby at 1,4 span columns 2
        text: "4 KM  ·  ONLINE"
        color: "#B9C0CA"
        size: 11
        bold: true
        wrap: false
        letterSpacing: 1
        alignX: "end"
        alignY: "center"

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 300
        maxHeight: 447
        backgroundColor: "#11151B"
        borderColor: "#252C36"
        borderWidth: 1
        radius: 28
        shadowColor: "#00000080"
        shadowBlur: 10
        shadowOffsetY: 5

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "#FFFFFF"
        size: 34
        bold: true
        wrap: false
        alignY: "center"

    Text matchBadge at 4,4 span columns 2
        text: "98% MATCH"
        color: "#FFFFFF"
        size: 9
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 4
        minWidth: 96
        minHeight: 30
        maxHeight: 30
        backgroundColor: "#FF4D67"
        borderColor: "#FF9AAA"
        borderWidth: 1
        radius: radiusPill

    Text profileMeta at 4,1 span columns 3
        text: "Chestnut  ·  Mare  ·  16.1 hands"
        color: "#C4CBD5"
        size: 13
        bold: true
        wrap: false
        alignY: "center"

    Text bio at 5,1 span columns 5
        text: "Beach gallops, shares hay, and emotionally available. Photobomber included."
        color: "#EEF1F5"
        size: 15
        wrap: true
        maxLines: 2
        lineHeightPercent: 140
        padding: 14
        backgroundColor: "#11161D"
        borderColor: "#303A47"
        borderWidth: 1
        radius: 18

    Button pass at 6,2
        text: "✕"
        visible: decision == ""
        size: 30
        tooltip: "Pass Buttercup"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 76
        maxWidth: 76
        minHeight: 76
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        focusable: !windowIsCompact
        padding: 0
        radius: 38
        backgroundColor: "#151B23"
        borderColor: "#515D6C"
        borderWidth: 1
        shadowColor: "#00000080"
        shadowBlur: 6
        shadowOffsetY: 3
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onPress: decision => "passed"

    Button like at 6,4
        visible: decision == ""
        text: "♥︎"
        size: 36
        tooltip: "Like Buttercup"
        accessibilityLabel: "Like Buttercup"
        minWidth: 76
        maxWidth: 76
        minHeight: 76
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        focusable: !windowIsCompact
        padding: 0
        radius: 38
        backgroundColor: "#FF4D67"
        borderColor: "#FF8FA1"
        borderWidth: 1
        shadowColor: "#00000080"
        shadowBlur: 8
        shadowOffsetY: 3
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onPress: decision => "liked"

    Text passLabel at 7,2
        text: "PASS"
        visible: decision == ""
        color: "#7F8996"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        layoutTransitionMs: motionNormal

    Text likeLabel at 7,4
        text: "LIKE"
        visible: decision == ""
        color: "#FF6B80"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        layoutTransitionMs: motionNormal

    Text result at 6,1 span columns 5
        text: "IT'S A MATCH  ·  Buttercup likes you too."
        visible: decision == "liked"
        color: "#FFE8EC"
        size: 13
        bold: true
        wrap: true
        maxLines: 2
        lineHeightPercent: 120
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        margin: 4
        padding: 10
        minHeight: 56
        maxHeight: 56
        backgroundColor: "#35141AF2"
        borderColor: "#A23649"
        borderWidth: 1
        radius: 16
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "PASSED  ·  The photobomber took that personally."
        visible: decision == "passed"
        color: "#DDE2E9"
        size: 12
        bold: true
        wrap: true
        maxLines: 2
        lineHeightPercent: 120
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        margin: 4
        padding: 10
        minHeight: 56
        maxHeight: 56
        backgroundColor: "#11161DF2"
        borderColor: "#465261"
        borderWidth: 1
        radius: 16
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#080A0E", surfaceRaisedColor: "#11161D", textColor: "#F7F8FA", textMutedColor: "#9AA1AD", accentColor: "#FF4D67", onAccentColor: "#FFFFFF", outlineColor: "#303945", shadowColor: "#00000080")
