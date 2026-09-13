view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    derived actionOffset: i64 = (windowWidth - 320) / 2
    derived edgeOffset: i64 = windowWidth - 320
    grid columns: 1fr 80 24 80 1fr
    grid rows: 46 1fr 54 34 68 80 auto
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
        color: "#AEB5C0"
        size: 10
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
        radius: 30
        shadowColor: "#00000099"
        shadowBlur: 12
        shadowOffsetY: 6

    Text profileName at 3,1 span columns 3
        text: "Buttercup, 7"
        color: "#FFFFFF"
        size: 36
        bold: true
        wrap: false
        alignY: "center"

    Text matchBadge at 4,4
        text: "98% MATCH"
        color: "#FFFFFF"
        size: 9
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        translateX: edgeOffset
        padding: 4
        minHeight: 26
        maxHeight: 26
        backgroundColor: "#FF4D67"
        borderColor: "#FF9AAA"
        borderWidth: 1
        radius: radiusPill

    Text profileMeta at 4,1 span columns 3
        text: "Chestnut  ·  Mare  ·  16.1 hands"
        color: "#B8C0CC"
        size: 12
        wrap: false
        alignY: "center"

    Text bio at 5,1 span columns 5
        text: "Beach gallops, generous with hay, and emotionally available. Photobomber included."
        color: "#EEF1F5"
        size: 15
        wrap: true
        maxLines: 2
        lineHeightPercent: 135
        padding: 12
        backgroundColor: "#11161D"
        borderColor: "#29313B"
        borderWidth: 1
        radius: 18

    Button pass at 6,2
        text: "✕"
        visible: !liked && !passed
        size: 36
        tooltip: "Pass"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 80
        maxWidth: 80
        minHeight: 80
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        focusable: !windowIsCompact
        padding: 0
        radius: 40
        backgroundColor: "#151B23"
        borderColor: "#465261"
        borderWidth: 2
        shadowColor: "#00000099"
        shadowBlur: 8
        shadowOffsetY: 4
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onPress: passed => !passed

    Button like at 6,4
        visible: !liked && !passed
        text: "♥︎"
        size: 38
        tooltip: "Like"
        accessibilityLabel: "Like Buttercup"
        minWidth: 80
        maxWidth: 80
        minHeight: 80
        alignX: "center"
        alignY: "center"
        translateX: actionOffset
        focusable: !windowIsCompact
        padding: 0
        radius: 40
        backgroundColor: "#FF4D67"
        borderColor: "#FF9AAA"
        borderWidth: 2
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 4
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onPress: liked => !liked

    Text passLabel at 7,2
        text: "PASS"
        visible: !liked && !passed
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
        visible: !liked && !passed
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
        text: "IT'S A MATCH  ·  Buttercup likes your pasture too."
        visible: liked
        color: "#FFE1E5"
        size: 13
        bold: true
        wrap: true
        maxLines: 2
        lineHeightPercent: 125
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        margin: 8
        padding: 16
        minHeight: 80
        maxHeight: 80
        backgroundColor: "#35141AF2"
        borderColor: "#8B3040"
        borderWidth: 1
        radius: 40
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "PASSED  ·  The photobomber is taking it personally."
        visible: passed
        color: "#D7DCE4"
        size: 12
        bold: true
        wrap: true
        maxLines: 2
        lineHeightPercent: 125
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        margin: 8
        padding: 16
        minHeight: 80
        maxHeight: 80
        backgroundColor: "#10151CF2"
        borderColor: "#3C4755"
        borderWidth: 1
        radius: 40
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#080A0E", surfaceRaisedColor: "#11161D", textColor: "#F7F8FA", textMutedColor: "#9AA1AD", accentColor: "#FF4D67", onAccentColor: "#FFFFFF", outlineColor: "#303945", shadowColor: "#00000080")
