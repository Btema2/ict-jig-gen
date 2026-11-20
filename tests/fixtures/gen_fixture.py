#!/usr/bin/env python3
"""Regenerate the synthetic EasyEDA-style fixture (run: python3 -I gen_fixture.py).

Board 60 x 40 mm at (100,50)..(160,90); 4 NPTH corner holes; 13 PTH pads:
R1, R2 (axial), D1, D2 (LED), U1 (3-pin regulator), J1 (2-pin power header).
"""
import os, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "demo_board")
os.makedirs(OUT, exist_ok=True)

X0, Y0, X1, Y1 = 100.0, 50.0, 160.0, 90.0

def g(v):  # mm -> 3:3 integer
    return str(round(v * 1000))

gko = "\r\n".join([
    "G04 Synthetic EasyEDA-style outline*",
    "%FSLAX33Y33*%", "%MOMM*%", "%ADD10C,0.254*%", "G01*", "D10*",
    f"X{g(X0)}Y{g(Y0)}D02*", f"X{g(X1)}Y{g(Y0)}D01*", f"X{g(X1)}Y{g(Y1)}D01*",
    f"X{g(X0)}Y{g(Y1)}D01*", f"X{g(X0)}Y{g(Y0)}D01*", "M02*", ""])

comps = {  # designator -> (footprint, centre, pad offsets)
    "R1": ("AXIAL-0.4", (115, 80), [(-2.54, 0), (2.54, 0)]),
    "R2": ("AXIAL-0.4", (115, 70), [(-2.54, 0), (2.54, 0)]),
    "D1": ("LED-5MM", (135, 80), [(-1.27, 0), (1.27, 0)]),
    "D2": ("LED-5MM", (135, 70), [(-1.27, 0), (1.27, 0)]),
    "U1": ("TO-92", (145, 60), [(-2.54, 0), (0, 0), (2.54, 0)]),
    "J1": ("HDR-1x2", (115, 60), [(-1.27, 0), (1.27, 0)]),
}
pads = [(round(c[0] + dx, 3), round(c[1] + dy, 3)) for _, c, offs in comps.values() for dx, dy in offs]
npth = [(105, 55), (155, 55), (105, 85), (155, 85)]

def drl(tool_dia, pts, comment):
    lines = ["M48", f";DRILL file {{{comment}}}", ";FORMAT={-:-/ absolute / metric / decimal}",
             "FMAT,2", "METRIC,TZ", f"T1C{tool_dia:.3f}", "%", "G90", "G05", "M71", "T1"]
    lines += [f"X{x:.3f}Y{y:.3f}" for x, y in pts]
    lines += ["T0", "M30", ""]
    return "\r\n".join(lines)

pth = drl(0.9, pads, "EasyEDA synthetic PTH")
npth_txt = drl(2.2, npth, "EasyEDA synthetic NPTH")

hdr = "Designator\tFootprint\tMid X\tMid Y\tRef X\tRef Y\tPad X\tPad Y\tLayer\tRotation\tComment"
rows = [hdr]
for d, (fp, (cx, cy), offs) in comps.items():
    px, py = cx + offs[0][0], cy + offs[0][1]
    rows.append(f"{d}\t{fp}\t{cx:.4f}mm\t{cy:.4f}mm\t{cx:.4f}mm\t{cy:.4f}mm\t{px:.4f}mm\t{py:.4f}mm\tT\t0\t{d}")
pnp = "\r\n".join(rows) + "\r\n"

files = {
    "Gerber_BoardOutlineLayer.GKO": gko,
    "Drill_PTH_Through.DRL": pth,
    "Drill_NPTH_Through.DRL": npth_txt,
    # layers the tool must ignore
    "Gerber_TopLayer.GTL": "G04 copper, ignored*\r\n%FSLAX33Y33*%\r\n%MOMM*%\r\nM02*\r\n",
    "Gerber_TopSilkscreenLayer.GTO": "G04 silk, ignored*\r\nM02*\r\n",
}
for name, text in files.items():
    with open(os.path.join(OUT, name), "w", newline="") as f:
        f.write(text)
with open(os.path.join(OUT, "PickAndPlace_demo_board.csv"), "w", newline="", encoding="utf-8") as f:
    f.write(pnp)
with open(os.path.join(HERE, "PickAndPlace_demo_board.csv"), "w", newline="", encoding="utf-8") as f:
    f.write(pnp)

zpath = os.path.join(HERE, "demo_board_gerber.zip")
with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
    for name in files:  # fixed timestamp keeps the archive reproducible
        zi = zipfile.ZipInfo(name, date_time=(2024, 1, 1, 0, 0, 0))
        zi.compress_type = zipfile.ZIP_DEFLATED
        z.writestr(zi, open(os.path.join(OUT, name), "rb").read())
print("pads:", len(pads), "npth:", len(npth))
