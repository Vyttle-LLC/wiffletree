"""Render the outlined brand SVGs without a browser or installed fonts."""
from pathlib import Path
from PIL import Image
from resvg_py import svg_to_bytes

ROOT = Path(__file__).resolve().parent.parent
PNG = ROOT / 'png'


def render(source, destination, width):
    destination.write_bytes(svg_to_bytes(svg_path=str(source), width=width,
                                        skip_system_fonts=True))


def main():
    PNG.mkdir(exist_ok=True)
    for name in ('deep-teal', 'stone', 'sea-glass', 'tide'):
        for width in (512, 1024):
            render(ROOT / f'icon/icon-{name}.svg', PNG / f'icon-{name}-{width}.png', width)
    for width in (16, 32, 48):
        render(ROOT / 'icon/icon-deep-teal-small.svg', PNG / f'favicon-{width}.png', width)
    for name, width in [('apple-touch-icon-180', 180), ('icon-192', 192)]:
        render(ROOT / 'icon/icon-deep-teal.svg', PNG / f'{name}.png', width)
    for source in sorted((ROOT / 'logo').glob('*.svg')):
        render(source, PNG / f'{source.stem}.png', 1600)
    render(ROOT / 'preview.svg', PNG / 'preview.png', 1600)
    images = [Image.open(PNG / f'favicon-{width}.png') for width in (16, 32, 48)]
    images[-1].save(ROOT / 'icon/favicon.ico', sizes=[(16,16), (32,32), (48,48)],
                    append_images=images[:-1], bitmap_format='png')
    print('PNG exports and favicon regenerated')


if __name__ == '__main__':
    main()
