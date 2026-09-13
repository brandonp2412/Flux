view HorseTinder {
    state decision: str = ""
    grid columns: 1fr 80 40 80 1fr
    grid rows: 50 1fr 60 36 auto 80
    grid gap: 10
    grid padding: 18
    grid scroll: true
    grid overlay: true

    Text brand at 1,1 span columns 3
        text: "HORSE TINDER"
        color: "#EAF6F5"
        size: 16
        bold: true
        wrap: false
        letterSpacing: 2
        alignY: "center"

    Text nearby at 1,4 span columns 2
        text: "LIVE  •  4 KM"
        color: "#A7C5C4"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        backgroundColor: "#102427"
        borderColor: "#285156"
        borderWidth: 1
        radius: radiusPill

    Image profilePhoto at 2,1 span columns 5
        source: "assets/buttercup.jpg"
        alt: "Buttercup, a chestnut horse, with another horse making a ridiculous face in the background"
        fit: "cover"
        clip: true
        minHeight: 405
        maxHeight: 485
        backgroundColor: "#0B1719"
        borderColor: "#24454A"
        borderWidth: 1
        radius: 24
        shadowColor: "#000000A6"
        shadowBlur: 16
        shadowOffsetY: 7

    Text profileName at 3,1 span columns 5
        text: "Buttercup, 7"
        color: "#F4FBFA"
        size: 34
        bold: true
        wrap: false
        alignY: "center"

    Text profileMeta at 4,1 span columns 3
        text: "CHESTNUT  •  MARE  •  16.1H"
        color: "#94AFB0"
        size: 10
        bold: true
        wrap: false
        letterSpacing: 1
        alignY: "center"

    Text matchBadge at 4,4 span columns 2
        text: "98% COMPATIBLE"
        color: "#082321"
        size: 9
        bold: true
        wrap: false
        letterSpacing: 1
        textAlign: "center"
        alignX: "end"
        alignY: "center"
        padding: 8
        minWidth: 112
        minHeight: 32
        maxHeight: 32
        backgroundColor: "#64D1C6"
        borderColor: "#A8F0E9"
        borderWidth: 1
        radius: radiusPill

    Text bio at 5,1 span columns 5
        text: "Beach gallops. Elite hay sharing. Emotionally available. Background horse has no boundaries."
        color: "#DCE9E9"
        size: 14
        wrap: true
        maxLines: 2
        lineHeightPercent: 145
        padding: 15
        backgroundColor: "#0E1C1FF2"
        borderColor: "#29454A"
        borderWidth: 1
        radius: 16

    Image pass at 6,2
        source: "assets/icon-pass.png"
        alt: "Pass"
        fit: "contain"
        visible: decision == ""
        accessibilityLabel: "Pass on Buttercup"
        minWidth: 78
        minHeight: 78
        padding: 20
        alignX: "center"
        alignY: "center"
        clip: true
        backgroundColor: "#102126"
        borderColor: "#446369"
        borderWidth: 1
        radius: 24
        shadowColor: "#00000080"
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
        minWidth: 78
        minHeight: 78
        padding: 19
        alignX: "center"
        alignY: "center"
        clip: true
        backgroundColor: "#64D1C6"
        borderColor: "#9AE8E1"
        borderWidth: 1
        radius: 24
        shadowColor: "#64D1C659"
        shadowBlur: 13
        shadowOffsetY: 5
        transitionMs: motionFast
        layoutTransitionMs: motionNormal
        onTap: decision => "liked"

    Text result at 6,1 span columns 5
        text: "98% MATCH  •  Buttercup is interested"
        visible: decision == "liked"
        color: "#CBF7F2"
        size: 14
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#123A3AF2"
        borderColor: "#3C8884"
        borderWidth: 1
        radius: 16
        layoutTransitionMs: motionNormal

    Text passedNote at 6,1 span columns 5
        text: "Passed  •  Returning to nearby profiles"
        visible: decision == "passed"
        color: "#C3D1D2"
        size: 13
        bold: true
        wrap: true
        textAlign: "center"
        alignX: "center"
        alignY: "center"
        padding: 16
        minHeight: 62
        backgroundColor: "#102126F2"
        borderColor: "#365158"
        borderWidth: 1
        radius: 16
        layoutTransitionMs: motionNormal
}

app HorseTinder(title: "Horse Tinder", width: 640, height: 900, resizable: true, theme: "dark", surfaceColor: "#071113", surfaceRaisedColor: "#0E1C1F", textColor: "#EAF6F5", textMutedColor: "#94AFB0", accentColor: "#64D1C6", onAccentColor: "#082321", outlineColor: "#29454A", shadowColor: "#00000099")
