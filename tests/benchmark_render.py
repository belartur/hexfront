"""Deterministic renderer benchmark, not a timing-sensitive unit test.

Run from the repository root: python3 -m tests.benchmark_render
Reports full draw_world CPU time, including moving units and overlays.
Use --profile to print the most expensive calls; --frames controls samples.
"""

import argparse
import cProfile
import os
import pstats
import statistics
import time
from types import SimpleNamespace

os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
os.environ.setdefault("SDL_AUDIODRIVER", "dummy")

import pygame

from hexfront import constants as C
from hexfront.board import Board
from hexfront.camera import Camera
from hexfront.entities import Building, BuildingKind, Vehicle
from hexfront.render import Renderer


def benchmark(frames: int, profile: bool, modes: tuple = ("fixed", "pan", "zoom"),
              zooms: tuple = (0.5, 1.0, 2.0)) -> None:
    """Measure fixed-view, fractional-pan and changing-zoom frames."""
    pygame.init()
    screen = pygame.display.set_mode((1180, 720))
    board = Board(256, 256)
    # Terraces exercise cliffs, while most of the board remains level.
    for (q, r), tile in board.tiles.items():
        tile.height = 1 + (q // 8 + r // 8) % 3
    board.set_ramp((127, 128), (128, 128), (126, 127))
    buildings = [Building(BuildingKind.BASE_TANK, i % 4, 124 + i, 128, 20)
                 for i in range(8)]
    vehicles = [Vehicle(list(C.VehicleKind)[i % 4], i % 4, 10, [],
                        board.center_world((124 + i % 8, 125 + i // 8)))
                for i in range(32)]
    starts = [v.pos for v in vehicles]
    scene = SimpleNamespace(board=board, buildings=buildings, vehicles=vehicles,
                            projectiles=[])
    profiler = cProfile.Profile()
    for mode in modes:
        for zoom in zooms:
            renderer = Renderer(screen)
            camera = Camera(screen.get_size())
            camera.center_on_world(*board.center_world((128, 128)))
            camera.zoom = zoom
            start = time.perf_counter()
            renderer.draw_world(scene, camera)
            cold = (time.perf_counter() - start) * 1000
            samples = []
            if profile:
                profiler.enable()
            for frame in range(frames):
                for vehicle, (x, y) in zip(vehicles, starts):
                    vehicle.x, vehicle.y = x + frame * 0.5, y
                if mode == "pan":
                    camera.x += 3.25
                    camera.y += 1.75
                elif mode == "zoom":
                    camera.zoom = zoom * (1 + (frame % 5) * 0.01)
                start = time.perf_counter()
                renderer.draw_world(scene, camera)
                samples.append((time.perf_counter() - start) * 1000)
            if profile:
                profiler.disable()
            print(f"{mode:5} zoom={zoom:.1f} cold={cold:.1f} ms "
                  f"median={statistics.median(samples):.1f} ms "
                  f"max={max(samples):.1f} ms", flush=True)
    if profile:
        pstats.Stats(profiler).sort_stats("cumtime").print_stats(20)
    pygame.quit()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--frames", type=int, default=10)
    parser.add_argument("--profile", action="store_true")
    parser.add_argument("--mode", choices=("fixed", "pan", "zoom"))
    parser.add_argument("--zoom", type=float, choices=(0.5, 1.0, 2.0))
    args = parser.parse_args()
    if args.frames < 1:
        parser.error("--frames must be positive")
    benchmark(args.frames, args.profile,
              (args.mode,) if args.mode else ("fixed", "pan", "zoom"),
              (args.zoom,) if args.zoom else (0.5, 1.0, 2.0))
