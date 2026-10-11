#!/usr/bin/env python3
"""生成 DMG 安装窗口背景图。

    python3 scripts/make-dmg-background.py

产物：src-tauri/dmg/background.png，用于「拖拽到 Applications」的安装指引。

Finder 显示 DMG 背景时按「像素 ÷ (DPI ÷ 72)」换算成点，所以这里输出
逻辑尺寸 660×420 的 2 倍图（1320×840 像素）并把 DPI 标为 144：
换算结果正好是 660×420 点，铺满 dmg.windowSize，且在 Retina 屏上按原生 2x 渲染。
（DPI 必须显式写入，否则 Finder 会按导出工具的默认值缩放着画。）

背景只画装饰、产品名与箭头：应用图标与「Applications」文件夹由 Finder 自己绘制，
因此图标所在的位置（x≈180 / x≈480，y≈205）必须留白。
"""
import os
from PIL import Image, ImageDraw, ImageFilter, ImageFont

# 逻辑尺寸（与 dmg.windowSize 对应，单位：点）
W, H = 660, 420
S = 4  # 超采样倍数：先按 4 倍画再缩小，保证线条锐利
R = 2  # 输出倍率：2x 图 + 144 DPI，Retina 屏上为原生分辨率

APP_X, APP_Y = 180, 205      # 应用图标中心（Finder 绘制，此处留白）
FOLDER_X, FOLDER_Y = 480, 205  # Applications 文件夹中心

BG_TOP = (11, 15, 24)
BG_BOTTOM = (15, 21, 36)
ACCENT = (77, 107, 254)
ACCENT2 = (0, 211, 167)
TEXT = (233, 238, 251)
MUTED = (127, 140, 168)
DIM = (94, 106, 132)
ARROW = (152, 164, 192)


def load_font(candidates, size):
    for path, index in candidates:
        if os.path.exists(path):
            try:
                return ImageFont.truetype(path, size, index=index)
            except Exception:
                continue
    return ImageFont.load_default()


BOLD = [
    ("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 0),
    ("/System/Library/Fonts/Supplemental/Helvetica Bold.ttf", 0),
    ("/System/Library/Fonts/HelveticaNeue.ttc", 1),
    ("/System/Library/Fonts/SFNS.ttf", 0),
]
REGULAR = [
    ("/System/Library/Fonts/Supplemental/Arial.ttf", 0),
    ("/System/Library/Fonts/HelveticaNeue.ttc", 0),
    ("/System/Library/Fonts/SFNS.ttf", 0),
]
CJK_REGULAR = [
    ("/System/Library/Fonts/PingFang.ttc", 12),
    ("/System/Library/Fonts/PingFang.ttc", 4),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Songti.ttc", 0),
]


def vertical_gradient(size, top, bottom):
    w, h = size
    grad = Image.new("RGB", (1, h))
    px = grad.load()
    for y in range(h):
        t = y / max(1, h - 1)
        px[0, y] = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
    return grad.resize((w, h), Image.BILINEAR)


def glow(size, center, radius, color, alpha):
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    steps = 48
    for i in range(steps, 0, -1):
        r = radius * i / steps
        a = int(alpha * (1 - i / steps) ** 2)
        if a <= 0:
            continue
        d.ellipse([center[0] - r, center[1] - r, center[0] + r, center[1] + r], fill=color + (a,))
    return layer.filter(ImageFilter.GaussianBlur(radius * 0.3))


def brand_mark(size, color):
    """产品标记：三根高低柱 + 外圈细环（对应前端 TL.brandSvg）。"""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    u = size / 24.0
    for x, h in ((3.0, 5.0), (7.6, 8.5), (12.2, 13.0)):
        d.rectangle([x * u, (17.5 - h) * u, (x + 3.2) * u, 17.5 * u], fill=color)
    r = 9.2 * u
    d.ellipse([12 * u - r, 12 * u - r, 12 * u + r, 12 * u + r],
              outline=color, width=max(1, int(1.6 * u)))
    return img


def paste_rotated(base, mark, center, angle):
    rot = mark.rotate(angle, expand=True, resample=Image.BICUBIC)
    base.alpha_composite(rot, (int(center[0] - rot.width / 2), int(center[1] - rot.height / 2)))


def text_width(draw, s, font, tracking=0):
    return sum(draw.textlength(ch, font=font) for ch in s) + tracking * max(0, len(s) - 1)


def draw_tracked(draw, cx, cy, s, font, fill, tracking=0):
    total = text_width(draw, s, font, tracking)
    x = cx - total / 2
    for ch in s:
        draw.text((x, cy), ch, font=font, fill=fill, anchor="lm")
        x += draw.textlength(ch, font=font) + tracking


def bezier(p0, p1, p2, t):
    u = 1 - t
    return (
        u * u * p0[0] + 2 * u * t * p1[0] + t * t * p2[0],
        u * u * p0[1] + 2 * u * t * p1[1] + t * t * p2[1],
    )


def build():
    size = (W * S, H * S)
    base = vertical_gradient(size, BG_TOP, BG_BOTTOM).convert("RGBA")

    base.alpha_composite(glow(size, (W * S * 0.14, -H * S * 0.10), W * S * 0.46, ACCENT, 70))
    base.alpha_composite(glow(size, (W * S * 0.98, H * S * 1.02), W * S * 0.5, ACCENT2, 52))
    base.alpha_composite(glow(size, (W * S * 0.62, H * S * 0.04), W * S * 0.3, ACCENT, 26))

    # 淡化装饰：产品标记 + 圆角矩形轮廓 + 圆环
    for cx, cy, s, ang, a in (
        (58, 62, 150, 16, 17),
        (604, 58, 132, -14, 15),
        (46, 362, 118, -22, 13),
        (616, 358, 142, 20, 14),
    ):
        paste_rotated(base, brand_mark(int(s * S), (255, 255, 255, a)), (cx * S, cy * S), ang)

    deco = Image.new("RGBA", size, (0, 0, 0, 0))
    dd = ImageDraw.Draw(deco)
    for cx, cy, box, ang, a in (
        (300, 58, 74, 12, 12),
        (96, 302, 86, -10, 13),
        (566, 300, 82, 14, 13),
    ):
        b = box * S
        layer = Image.new("RGBA", (b, b), (0, 0, 0, 0))
        ImageDraw.Draw(layer).rounded_rectangle(
            [2, 2, b - 2, b - 2], radius=int(b * 0.26),
            outline=(255, 255, 255, a), width=max(2, int(1.6 * S)))
        rot = layer.rotate(ang, expand=True, resample=Image.BICUBIC)
        deco.alpha_composite(rot, (int(cx * S - rot.width / 2), int(cy * S - rot.height / 2)))
    for cx, cy, r, a in ((330, 128, 34, 11), (250, 356, 26, 10)):
        dd.ellipse([(cx - r) * S, (cy - r) * S, (cx + r) * S, (cy + r) * S],
                   outline=(255, 255, 255, a), width=max(2, int(1.6 * S)))
    base.alpha_composite(deco)

    draw = ImageDraw.Draw(base)

    # 产品名 + 副标题
    draw_tracked(draw, W * S / 2, 66 * S, "TokenSonar",
                 load_font(BOLD, int(38 * S)), TEXT, tracking=0.6 * S)
    draw_tracked(draw, W * S / 2, 98 * S, "AI  USAGE  CONSOLE",
                 load_font(REGULAR, int(11 * S)), DIM, tracking=2.6 * S)

    # 曲线箭头：指向 Applications
    p0 = (252 * S, 232 * S)
    p1 = (330 * S, 168 * S)
    p2 = (406 * S, 214 * S)
    pts = [bezier(p0, p1, p2, i / 60.0) for i in range(61)]
    draw.line(pts, fill=ARROW + (235,), width=int(3.2 * S), joint="curve")
    draw.ellipse([p0[0] - 2.6 * S, p0[1] - 2.6 * S, p0[0] + 2.6 * S, p0[1] + 2.6 * S],
                 fill=ARROW + (235,))
    # 箭头：按末端切线方向画实心三角
    tx, ty = p2[0] - pts[-6][0], p2[1] - pts[-6][1]
    n = (tx * tx + ty * ty) ** 0.5 or 1
    ux, uy = tx / n, ty / n
    nx, ny = -uy, ux
    L, Wd = 17 * S, 6.4 * S
    tip = (p2[0] + ux * L * 0.5, p2[1] + uy * L * 0.5)
    bl = (p2[0] - ux * L * 0.5 + nx * Wd, p2[1] - uy * L * 0.5 + ny * Wd)
    br = (p2[0] - ux * L * 0.5 - nx * Wd, p2[1] - uy * L * 0.5 - ny * Wd)
    draw.polygon([tip, bl, br], fill=ARROW + (245,))

    # 安装指引（有中文字体时双语，否则英文）
    cjk = load_font(CJK_REGULAR, int(12.5 * S))
    has_cjk = getattr(cjk, "path", None) is not None and os.path.exists(
        getattr(cjk, "path", "") or "")
    if not has_cjk:
        # load_font 回退到默认位图字体时 path 为空，这里再兜底探测一次
        has_cjk = os.path.exists("/System/Library/Fonts/PingFang.ttc")
    if has_cjk:
        draw_tracked(draw, W * S / 2, 352 * S,
                     "拖入右侧「应用程序」文件夹，即可完成安装",
                     cjk, MUTED, tracking=0.3 * S)
    draw_tracked(draw, W * S / 2, 372 * S,
                 "Drag the app into the Applications folder to install",
                 load_font(REGULAR, int(10.5 * S)), DIM, tracking=0.2 * S)

    out = base.convert("RGB").resize((W * R, H * R), Image.LANCZOS)
    dest_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                            "src-tauri", "dmg")
    os.makedirs(dest_dir, exist_ok=True)
    dest = os.path.join(dest_dir, "background.png")
    out.save(dest, "PNG", optimize=True, dpi=(72 * R, 72 * R))
    print("wrote", dest, out.size, "dpi", 72 * R)


if __name__ == "__main__":
    build()
