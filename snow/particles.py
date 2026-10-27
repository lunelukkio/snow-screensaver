from __future__ import annotations

import math
import random

from PIL import Image, ImageDraw, ImageFilter, ImageTk


SWAY_FREQ = 0.8       # radians per second
SUPERSAMPLE = 4       # for crisp edges before gaussian blur


def render_flake_pil(size: int, edge_blur: int, color: str) -> Image.Image:
    """Soft-edged flake rendered to RGBA. Larger box for the blur halo."""
    pad = max(2, edge_blur * 2)
    box = size + pad * 2
    big = box * SUPERSAMPLE
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    h = color.lstrip("#")
    r, g, b = int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)
    bs = size * SUPERSAMPLE
    bp = pad * SUPERSAMPLE
    draw.ellipse((bp, bp, bp + bs, bp + bs), fill=(r, g, b, 255))
    img = img.resize((box, box), Image.LANCZOS)
    if edge_blur > 0:
        img = img.filter(ImageFilter.GaussianBlur(edge_blur))
    return img


class FlakeField:
    """Per-monitor preallocated particle arrays, drawn as image items."""

    def __init__(self, *, width: int, height: int, count: int, size: int,
                 speed: float, edge_blur: int, sway_amp: int, color: str) -> None:
        self.width = width
        self.height = height
        self.count = count
        self.size = size
        self.speed = speed
        self.edge_blur = edge_blur
        self.sway_amp = sway_amp
        self.color = color

        n = count
        self.base_x = [random.uniform(0, width) for _ in range(n)]
        self.x = list(self.base_x)
        self.y = [random.uniform(-height, height) for _ in range(n)]
        self.vy = [speed * random.uniform(0.7, 1.3) for _ in range(n)]
        self.phase = [random.uniform(0, math.tau) for _ in range(n)]
        self.img_id: list[int] = [0] * n
        self._photo: ImageTk.PhotoImage | None = None  # held to prevent GC

    def _refresh_photo(self) -> None:
        pil = render_flake_pil(self.size, self.edge_blur, self.color)
        self._photo = ImageTk.PhotoImage(pil)

    def create_items(self, canvas) -> None:
        self._refresh_photo()
        half = self.size / 2
        for i in range(self.count):
            self.img_id[i] = canvas.create_image(
                self.x[i] + half, self.y[i] + half, image=self._photo
            )

    def step(self, canvas, dt: float, t: float) -> None:
        x = self.x
        y = self.y
        vy = self.vy
        phase = self.phase
        base_x = self.base_x
        img_id = self.img_id
        s = self.size
        h = self.height
        w = self.width
        coords = canvas.coords
        sin = math.sin
        amp = self.sway_amp
        freq = SWAY_FREQ
        half = s / 2

        for i in range(self.count):
            ny = y[i] + vy[i] * dt * 60.0
            if ny > h:
                ny = -s
                base_x[i] = random.uniform(0, w)
                phase[i] = random.uniform(0, math.tau)
            nx = base_x[i] + sin(phase[i] + t * freq) * amp
            x[i] = nx
            y[i] = ny
            coords(img_id[i], nx + half, ny + half)

    # ---- live mutators ----

    def _reapply_image(self, canvas) -> None:
        self._refresh_photo()
        for iid in self.img_id:
            canvas.itemconfig(iid, image=self._photo)

    def set_speed(self, speed: float) -> None:
        self.speed = speed
        for i in range(self.count):
            self.vy[i] = speed * random.uniform(0.7, 1.3)

    def set_size(self, canvas, size: int) -> None:
        if size == self.size:
            return
        self.size = size
        self._reapply_image(canvas)

    def set_color(self, canvas, color: str) -> None:
        self.color = color
        self._reapply_image(canvas)

    def set_edge_blur(self, canvas, edge_blur: int) -> None:
        if edge_blur == self.edge_blur:
            return
        self.edge_blur = edge_blur
        self._reapply_image(canvas)

    def set_sway_amp(self, sway_amp: int) -> None:
        self.sway_amp = sway_amp

    def set_count(self, canvas, count: int) -> None:
        if count == self.count:
            return
        if count < self.count:
            for i in range(count, self.count):
                canvas.delete(self.img_id[i])
            del self.x[count:]
            del self.y[count:]
            del self.vy[count:]
            del self.phase[count:]
            del self.base_x[count:]
            del self.img_id[count:]
        else:
            s = self.size
            half = s / 2
            for _ in range(count - self.count):
                bx = random.uniform(0, self.width)
                self.base_x.append(bx)
                self.x.append(bx)
                self.y.append(random.uniform(-self.height, 0))
                self.vy.append(self.speed * random.uniform(0.7, 1.3))
                self.phase.append(random.uniform(0, math.tau))
                iid = canvas.create_image(bx + half, self.y[-1] + half, image=self._photo)
                self.img_id.append(iid)
        self.count = count
