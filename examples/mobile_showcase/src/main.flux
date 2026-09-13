view HorseTinder {
    state decision: str = ""
    grid columns: 1fr 80 40 80 1fr
    grid rows: 52 1fr 60 36 auto 80
    grid gap: 12
    grid padding: 20
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "HORSE TINDER"
        color: "#F7F8FA"
        size: 17
        bold: true
        wrap: false
        letterSpacing: 2
        alignY: "center"

    Text nearby at 1,4 span columns 2
        text: "4 KM  •  ONLINE"
        color: "#8E97A6"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#11161D"
        borderColor: "#242C37"
        borderWidth: 1
        radius: radiusPill

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 390
        maxHeight: 470
        backgroundColor: "#10141A"
        borderColor: "#242A33"
        borderWidth: 1
        radius: 30
        shadowColor: "#00000099"
        shadowBlur: 14
        shadowOffsetY: 7

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "#FFFFFF"
        size: 36
        bold: true
        wrap: false
        alignY: "center"

    Text matchBadge at 4,4 span columns 2
        text: "98% MATCH"
        color: "#FFFFFF"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 10
        minWidth: 104
        minHeight: 36
        maxHeight: 36
        backgroundColor: "#FF4D67"
        borderColor: "#FF92A3"
        borderWidth: 1
        radius: radiusPill

    Text profileMeta at 4,1 span columns 3
        text: "CHESTNUT  •  MARE  •  16.1 HANDS"
        color: "#AAB2BF"
        size: 11
        bold: true
        wrap: false
        letterSpacing: 1
        alignY: "center"

    Text bio at 5,1 span columns 5
        text: "Beach gallops, shares hay, and emotionally available. Photobomber included."
        color: "#E8ECF2"
        size: 15
        wrap: true
        maxLines: 2
        lineHeightPercent: 145
        padding: 16
        backgroundColor: "#10151C"
        borderColor: "#29323E"
        borderWidth: 1
        radius: 20

    Image pass at 6,2
        source: "assets/icon-pass.png"
        alt: "Pass"
        fit: "contain"
        visible: decision == ""
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 80
        minHeight: 80
        padding: 20
        alignX: "center"
        alignY: "center"
        clip: true
        backgroundColor: "#151B23"
        borderColor: "#536071"
        borderWidth: 1
        radius: 40
        shadowColor: "#00000099"
        shadowBlur: 8
        shadowOffsetY: 4
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "passed"

    Image like at 6,4
        source: "assets/icon-heart.png"
        alt: "Like"
        fit: "contain"
        visible: decision == ""
        accessibilityLabel: "Like Buttercup"
        minWidth: 80
        minHeight: 80
        padding: 19
        alignX: "center"
        alignY: "center"
        clip: true
        backgroundColor: "#FF4D67"
        borderColor: "#FF9BAD"
        borderWidth: 1
        radius: 40
        shadowColor: "#FF4D6759"
        shadowBlur: 14
        shadowOffsetY: 5
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "liked"

    Text result at 6,1 span columns 5
        text: "MATCHED  •  Buttercup likes you too"
        visible: decision == "liked"
        color: "#FFE9ED"
        size: 14
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#35151CF2"
        borderColor: "#A13B4E"
        borderWidth: 1
        radius: 20
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "PASSED  •  The photobomber will remember this"
        visible: decision == "passed"
        color: "#D9DFE7"
        size: 13
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#121820F2"
        borderColor: "#3A4655"
        borderWidth: 1
        radius: 20
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#07090D", surfaceRaisedColor: "#10151C", textColor: "#F7F8FA", textMutedColor: "#9AA3B0", accentColor: "#FF4D67", onAccentColor: "#FFFFFF", outlineColor: "#29323E", shadowColor: "#00000099")
