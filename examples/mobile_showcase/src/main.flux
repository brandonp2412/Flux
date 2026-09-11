view HorseTinder {
    state liked: bool = false
    state passed: bool = false
    grid columns: 1fr 1fr
    grid rows: auto auto auto auto auto auto auto auto auto auto auto
    grid gap: 10
    grid padding: 16
    grid scroll: true

    Text brand at 1,1 span columns 2
        text: "HORSE TINDER  ·  NEIGHBORHOOD"
        color: "#BE185D"
        size: 12
        bold: true
        letterSpacing: 1

    Text title at 2,1 span columns 2
        text: "Find your stable relationship."
        color: "#111827"
        size: 28
        bold: true
        wrap: true
        maxLines: 2
        marginBottom: 2

    Text profile at 3,1 span columns 2
        text: "🐴\nBUTTERCUP, 7\nPalomino · 16.1 hands\n📍 4 km away"
        color: "#4A1D2F"
        size: 24
        bold: true
        textAlign: "center"
        wrap: true
        maxLines: 6
        padding: 18
        minHeight: 190
        backgroundColor: "#FCE7F3"
        borderColor: "#F9A8D4"
        borderWidth: 1
        radius: 28
        shadowColor: "#83184324"
        shadowBlur: 22
        shadowOffsetY: 8

    Text chemistry at 4,1 span columns 2
        text: "98% mane chemistry  ·  verified carrot enthusiast"
        color: "#9D174D"
        size: 14
        bold: true
        textAlign: "center"
        padding: 10
        backgroundColor: "#FDF2F8"
        radius: 16

    Text weekend at 5,1
        text: "WEEKEND\nBeach gallops\nthen snacks"
        color: "#1E3A8A"
        size: 16
        bold: true
        wrap: true
        padding: 12
        minHeight: 78
        backgroundColor: "#EFF6FF"
        radius: 18

    Text greenFlag at 5,2
        text: "GREEN FLAG\nShares hay\nwithout drama"
        color: "#14532D"
        size: 16
        bold: true
        wrap: true
        padding: 12
        minHeight: 78
        backgroundColor: "#F0FDF4"
        radius: 18

    Text bio at 6,1 span columns 2
        text: "About Buttercup\nEmotionally available. Great listener. Once jumped a tiny fence and has mentioned it at every dinner since."
        color: "#374151"
        size: 16
        wrap: true
        maxLines: 6
        padding: 14
        backgroundColor: "#FFFFFF"
        borderColor: "#E5E7EB"
        borderWidth: 1
        radius: 20

    Text prompt at 7,1 span columns 2
        text: "Ideal first date:  sunset trail, suspiciously expensive apples, home before the flies get weird."
        color: "#6B21A8"
        size: 15
        wrap: true
        maxLines: 4
        padding: 12
        backgroundColor: "#FAF5FF"
        radius: 18

    Button pass at 8,1
        text: "✕  Pass"
        padding: 14
        radius: 18
        backgroundColor: "#FFFFFF"
        borderColor: "#D1D5DB"
        borderWidth: 1
        onPress: passed => !passed

    Button like at 8,2
        text: "♥  Like"
        primary: true
        padding: 14
        radius: 18
        onPress: liked => !liked

    Text matched at 9,1 span columns 2
        text: "IT'S A MATCH  ·  Buttercup also likes your pasture situation."
        visible: liked
        color: "#9D174D"
        size: 15
        bold: true
        textAlign: "center"
        padding: 16
        backgroundColor: "#FCE7F3"
        radius: 18

    Text passedNote at 10,1 span columns 2
        text: "Passed. Somewhere, a horse has dramatically stared into the middle distance."
        visible: passed
        color: "#475569"
        size: 14
        textAlign: "center"
        padding: 14
        backgroundColor: "#F8FAFC"
        radius: 16

    Text footer at 11,1 span columns 2
        text: "No endless swiping. Just premium-grade horsing around."
        color: "#64748B"
        size: 12
        textAlign: "center"
        marginTop: 4
        marginBottom: 12
}

app HorseTinder(title: "Horse Tinder", theme: "light")
