view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 80 24 80 1fr
    grid rows: 50 406 62 34 60 76 auto
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
        minHeight: 396
        maxHeight: 406
        backgroundColor: "#11151B"
        borderColor: "#252C36"
        borderWidth: 1
        radius: 28
        shadowColor: "#00000099"
        shadowBlur: 12
        shadowOffsetY: 6

    Text profileName at 3,1 span columns 3
        text: "Buttercup, 7"
        color: "#FFFFFF"
        size: 34
        bold: true
        wrap: false
        alignY: "center"

    Text matchBadge at 3,4
        text: "98% MATCH"
        color: "#FFFFFF"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#FF4D67"
        borderColor: "#FF9AAA"
        borderWidth: 1
        radius: radiusPill

    Text profileMeta at 4,1 span columns 5
        text: "Chestnut  ·  Mare  ·  16.1 hands  ·  4 km away"
        color: "#C8CED7"
        size: 12
        bold: true
        wrap: false

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
        radius: radiusLg

    Button pass at 6,2
        text: "✕"
        size: 42
        tooltip: "Pass"
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 70
        maxWidth: 70
        minHeight: 70
        alignX: "center"
        alignY: "center"
        focusable: true
        padding: 0
        radius: 35
        backgroundColor: "#151B23"
        borderColor: "#465261"
        borderWidth: 2
        shadowColor: "#00000099"
        shadowBlur: 10
        shadowOffsetY: 5
        transitionMs: motionFast
        onPress: passed => !passed

    Button like at 6,4
        text: "♥︎"
        size: 47
        tooltip: "Like"
        accessibilityLabel: "Like Buttercup"
        minWidth: 74
        maxWidth: 74
        minHeight: 74
        alignX: "center"
        alignY: "center"
        focusable: true
        padding: 0
        radius: 37
        backgroundColor: "#FF4D67"
        borderColor: "#FF9AAA"
        borderWidth: 2
        shadowColor: "#00000099"
        shadowBlur: 12
        shadowOffsetY: 5
        transitionMs: motionFast
        onPress: liked => !liked

    Text result at 7,1 span columns 5
        text: "IT'S A MATCH  ·  Buttercup likes your pasture too."
        visible: liked
        color: "#FFE1E5"
        size: 14
        bold: true
        textAlign: "center"
        alignX: "center"
        margin: 10
        padding: 14
        backgroundColor: "#35141AF2"
        borderColor: "#8B3040"
        borderWidth: 1
        radius: 20
        layoutTransitionMs: motionNormal

    Text passedNote at 7,1 span columns 5
        text: "Passed. The photobomber is taking it personally."
        visible: passed
        color: "#E5E7EB"
        size: 13
        textAlign: "center"
        alignX: "center"
        margin: 10
        padding: 14
        backgroundColor: "#11151BF2"
        borderColor: "#343C47"
        borderWidth: 1
        radius: 20
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#080A0E", surfaceRaisedColor: "#11161D", textColor: "#F7F8FA", textMutedColor: "#9AA1AD", accentColor: "#FF4D67", onAccentColor: "#FFFFFF", outlineColor: "#303945", shadowColor: "#00000080")
