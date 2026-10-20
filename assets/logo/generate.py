"""Write BIMCraft's logo SVGs: the mark, the app tiles and the mark with the wordmark, each light and dark.

The outputs are committed beside this script, which writes them there: python3 assets/logo/generate.py
"""
import math
import os

OUT = os.path.dirname(os.path.abspath(__file__))
T = math.tan(math.radians(30))
BLUE = "#0A6CFF"
INK = "#1E2430"
WHITE = "#FFFFFF"


def f(v):
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s == "-0" else s


def pts(ps, dx, dy, k):
    return " ".join(f"{f(dx + x * k)},{f(dy + y * k)}" for x, y in ps)


# The mark, 230 wide and 300 tall: a stem, an isometric top face and the bowl's lower half.
STEM = [(0, 80 * T), (80, 0), (80, 300), (0, 300 - 80 * T)]
YB = 260 - 160 * T  # where the lower piece's top meets the face
FACE = [(80, YB - 130 * T), (130, YB - 80 * T - 100 * T), (230, YB - 80 * T), (150, YB), (80, YB - 70 * T)]
LOWER = [(150, YB), (230, YB + 80 * T), (230, 300 - 80 * T), (150, 300)]


def mark(dx, dy, k, solid, face, face_opacity=None):
    op = f' fill-opacity="{face_opacity}"' if face_opacity else ""
    return (
        f'  <polygon points="{pts(FACE, dx, dy, k)}" fill="{face}"{op}/>\n'
        f'  <polygon points="{pts(STEM, dx, dy, k)}" fill="{solid}"/>\n'
        f'  <polygon points="{pts(LOWER, dx, dy, k)}" fill="{solid}"/>\n'
    )


# The wordmark's strokes, 24 wide, on a baseline at 463 with the x-height at 349 and ascenders at 297.
def ring(cx, cy=406, r=45):
    return f"M{cx - r} {cy}a{r} {r} 0 1 0 {2 * r} 0a{r} {r} 0 1 0 {-2 * r} 0"


def c_arc(cx, cy=406, r=45, a=28):
    x, y = r * math.cos(math.radians(a)), r * math.sin(math.radians(a))
    return f"M{f(cx + x)} {f(cy - y)}A{r} {r} 0 1 0 {f(cx + x)} {f(cy + y)}"


WORD = " ".join([
    "M550 307V463", ring(598),                                   # b
    "M679 349V463",                                              # i
    "M720 463V349M720 398A37 37 0 0 1 794 398V463"
    "M794 398A37 37 0 0 1 868 398V463",                          # m
    c_arc(948),                                                  # c
    "M1026 463V349M1026 398A37 37 0 0 1 1063 361H1071",          # r
    ring(1130), "M1177 349V463",                                 # a
    "M1212 463V334A25 25 0 0 1 1237 309H1243M1200 361H1245",     # f
    "M1278 297V428A23 23 0 0 0 1301 451H1317M1254 361H1317",     # t
])
DOT = (679, 316, 13)


def svg(w, h, body, title):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" role="img">\n'
        f"  <title>{title}</title>\n{body}</svg>\n"
    )


def write(name, text):
    with open(f"{OUT}/{name}", "w") as fh:
        fh.write(text)


# The bare mark, for light and for dark backgrounds.
for theme, solid in (("light", INK), ("dark", WHITE)):
    write(f"bimcraft-mark-{theme}.svg", svg(230, 300, mark(0, 0, 1, solid, BLUE), "BIMCraft"))

# The app tile: blue with a white mark, or ink with a white mark and the blue face.
for theme, tile, face, op in (("light", BLUE, WHITE, "0.55"), ("dark", INK, BLUE, None)):
    k = 0.56
    body = f'  <rect x="8" y="8" width="240" height="240" rx="44" fill="{tile}"/>\n'
    body += mark(128 - 115 * k, 128 - 150 * k, k, WHITE, face, op)
    write(f"bimcraft-icon-{theme}.svg", svg(256, 256, body, "BIMCraft"))

# The mark with the wordmark, laid out as the mockup is: the mark from (231, 207), the letters from 538.
X0, Y0, W, H = 231, 207, 1086, 300
for theme, solid in (("light", INK), ("dark", WHITE)):
    body = mark(0, 0, 1, solid, BLUE)
    body += (
        f'  <g transform="translate({-X0} {-Y0})" fill="none" stroke="{solid}" stroke-width="24">\n'
        f'    <path d="{WORD}"/>\n'
        f'    <circle cx="{DOT[0]}" cy="{DOT[1]}" r="{DOT[2]}" fill="{solid}" stroke="none"/>\n'
        f"  </g>\n"
    )
    write(f"bimcraft-logo-{theme}.svg", svg(W, H, body, "BIMCraft"))
