view HorseTinder {
    state decision: str = ""
    grid columns: 1fr 80 40 80 1fr
    grid rows: 48 1fr 60 36 64 80
    grid gap: 10
    grid padding: 18
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "HORSE TINDER"
        color: "#FFF8F4"
        size: 17
        bold: true
        wrap: false
        letterSpacing: 2
        alignY: "center"

    Text nearby at 1,4 span columns 2
        text: "4 KM  /  ONLINE"
        color: "#B9B8B5"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 410
        maxHeight: 490
        backgroundColor: "#171614"
        borderColor: "#3A3834"
        borderWidth: 1
        radius: 18
        shadowColor: "#000000A6"
        shadowBlur: 12
        shadowOffsetY: 6

    Text profileName at 3,1 span columns 5
        text: "BUTTERCUP, 7"
        color: "#FFF9F5"
        size: 31
        bold: true
        wrap: false
        letterSpacing: 1
        alignY: "center"

    Text matchBadge at 4,4 span columns 2
        text: "98 / 100"
        color: "#2A120A"
        size: 11
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 9
        minWidth: 92
        minHeight: 36
        maxHeight: 36
        backgroundColor: "#FFB06B"
        borderColor: "#FFD2AA"
        borderWidth: 1
        radius: 10

    Text profileMeta at 4,1 span columns 3
        text: "CHESTNUT MARE  /  16.1 HANDS  /  ACTIVE NOW"
        color: "#AFAEAA"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        alignY: "center"

    Text bio at 5,1 span columns 5
        text: "Beach gallops · shares hay · emotionally available · chaotic friend included"
        color: "#E5E2DD"
        size: 13
        bold: true
        wrap: true
        maxLines: 2
        lineHeightPercent: 135
        textAlign: "center"
        alignY: "center"
        padding: 13
        backgroundColor: "#171614"
        borderColor: "#3A3834"
        borderWidth: 1
        radius: 12

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
        backgroundColor: "#1A1917"
        borderColor: "#595650"
        borderWidth: 1
        radius: 16
        shadowColor: "#00000099"
        shadowBlur: 7
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
        backgroundColor: "#FF6A3D"
        borderColor: "#FF9B7C"
        borderWidth: 1
        radius: 16
        shadowColor: "#FF6A3D52"
        shadowBlur: 12
        shadowOffsetY: 5
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "liked"

    Text result at 6,1 span columns 5
        text: "MATCH CONFIRMED  /  BUTTERCUP LIKES YOU"
        visible: decision == "liked"
        color: "#FFF0E9"
        size: 13
        bold: true
        wrap: true
        letterSpacing: 1
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#3A1B12F2"
        borderColor: "#A4492E"
        borderWidth: 1
        radius: 12
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "PASSED  /  NEXT PROFILE READY"
        visible: decision == "passed"
        color: "#D8D6D1"
        size: 13
        bold: true
        wrap: true
        letterSpacing: 1
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#181715F2"
        borderColor: "#4B4944"
        borderWidth: 1
        radius: 12
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#0D0C0B", surfaceRaisedColor: "#171614", textColor: "#FFF8F4", textMutedColor: "#AFAEAA", accentColor: "#FF6A3D", onAccentColor: "#FFFFFF", outlineColor: "#3A3834", shadowColor: "#00000099")
