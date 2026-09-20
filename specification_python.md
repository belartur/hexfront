# Hexfront — specyfikacja implementacji w Pythonie

## Wstęp
Ta specyfikacja opisuje implementację gry w Pythonie: PyGame obsługuje okno, wejście i rysowanie, a NumPy obliczenia pikselowe renderera. Kod i testy tej implementacji leżą w katalogu `python/`.

Część wspólną wszystkich implementacji (kontrakt wizualny, sterowanie, parametry, obsługa plansz) opisuje [specification.md](specification.md), zasady gry [rules.md](rules.md), binarny format planszy [specification_of_map_format.md](specification_of_map_format.md), a edytor plansz [specification_of_map_editor.md](specification_of_map_editor.md) — edytor na razie istnieje tylko w tej implementacji. Rozwiązania z tej specyfikacji obowiązują tylko tę implementację i nie są wzorcem dla pozostałych.

## Układ kodu
```text
python/main.py       punkt wejścia gry (python3 python/main.py)
python/editor.py     punkt wejścia edytora plansz (python3 python/editor.py [mapa])
python/make_maps.py  regeneruje przykładowe plansze z generatora do maps/
python/hexfront/     pakiet gry: logika i UI
python/tests/        testy headless (dummy video driver)
```

Moduły pakietu `python/hexfront/`:
* `constants.py` — wszystkie stałe możliwe do zmiany, każda z komentarzem wskazującym regułę z [rules.md](rules.md): `UNIT_J_TO_PX` jako jedyne miejsce przelicznika j → px, stałe symulacji (`FPS`, `SIM_DT`), stałe rzutu izometrycznego, `REPO_ROOT`, `MAPS_DIR`, `MAP_EXTENSION`;
* `hexgrid.py` — geometria sześciokątów flat-top (odd-q) bez logiki gry: sąsiedzi, przeliczenia heks ↔ świat, odległości, rogi, numeracja krawędzi;
* `board.py` — plansza: pola, utrudnienia (ściany, miny, pułapki), podjazdy, mosty, wyszukiwanie drogi, wskazywanie pola kursorem (tryb „płaski” z klawiszem Alt);
* `entities.py` — dane gry: rodzaje budynków, gracze, budynki, pojazdy i ich pojemności;
* `game.py` — symulacja czasu rzeczywistego o stałym kroku `SIM_DT`, bez zależności od PyGame: produkcja baz, przeludnienie, działka, wieże, aura bufora, ruch, walka, miny, pułapki, ściany, przejęcia budynków, eliminacja i zwycięstwo;
* `ai.py` — sterowanie przeciwnikami według sekcji 13 [rules.md](rules.md); deterministyczne dla danego seeda;
* `levels.py` — generator proceduralny plansz; korzystają z niego `make_maps.py` i testy;
* `mapfile.py` — zapis i odczyt formatu z [specification_of_map_format.md](specification_of_map_format.md) oraz lista poziomów dla menu;
* `camera.py` — rzut izometryczny i widok: przesuwanie, zoom, ograniczanie do planszy;
* `depth.py` — programowy bufor głębokości: rasteryzacja powierzchni i linii, zbiorczy test głębokości, przezroczyste cienie;
* `render.py` — rysowanie z kodu, bez assetów rastrowych, z cache nieruchomego terenu;
* `app.py` — okno, menu poziomów, obsługa wejścia i HUD.

## Uruchamianie i testy
Potrzebne są biblioteki PyGame i NumPy (np. `pip install pygame numpy`). Grę, edytor i testy można uruchamiać z dowolnego katalogu, bo ścieżki liczone są od katalogu głównego repozytorium.

```bash
python3 python/main.py           # gra
python3 python/editor.py [mapa]  # edytor plansz
python3 python/make_maps.py      # regeneracja maps/*.map z generatora
```

```bash
cd python && python3 -m tests.test_logic    # reguły gry (headless)
cd python && python3 -m tests.test_mapfile  # format plansz i edytor
cd python && python3 -m tests.test_render   # regresja renderingu (wymaga numpy)
cd python && python3 -m tests.benchmark_render   # benchmark renderera (--frames, --mode, --zoom)
```

## Renderowanie
Gra i edytor współdzielą renderer (`render.py`). Obliczenia pikselowe wykonuje NumPy (zależność uruchomieniowa obok PyGame); maski wypełnień pól i ich siatki rasteruje PyGame. Stała `ISO_SIN` z `python/hexfront/constants.py` jest współczynnikiem k rzutu izometrycznego z [specification.md](specification.md) (sekcja „Grafika i interfejs użytkownika”). Zachowany jest culling widoku — obliczenia prymitywów ograniczamy do ich prostokątów ekranowych, skarpy całkowicie poza ekranem odrzucamy przed projekcją wierzchołków (z uwzględnieniem pełnej wysokości ściany), a geometrię nieobecnych skarp i wierzchów poza ekranem pomijamy.

Obraz i głębokość nieruchomego terenu są buforowane. Cache unieważnia zmiana kamery, rozmiaru okna, planszy, wysokości lub ramp; ruchome obiekty i nakładki nie trafiają do cache. Przy pełnym przerysowaniu wierzchy pól i ich siatka przechodzą zbiorczy test głębokości dla każdej wysokości, bez osobnych tablic NumPy na każdą krawędź. Dla obu masek (wypełnień i siatki) głębokość pochodzi z równania tej samej płaszczyzny w środku piksela, również na brzegach zaokrąglonego obrysu. Zapis widocznych kolorów i głębokości odbywa się bezpośrednio pod maską NumPy, bez tworzenia tablic wybranych wartości. Zmiany widoku, także podpikselowe, są renderowane bez zaokrąglania kamery lub skalowania starej klatki. Zasięgi i oznaczenia interfejsu zachowują osobne przejścia nakładkowe, a liczniki jednostek rysowane są na końcu. Porównania głębokości wykonujemy z numeryczną tolerancją, żeby współpłaszczyznowe fragmenty nie migały.

Pełne przerysowanie po zmianie widoku jest droższe od klatki z nieruchomą kamerą; ta implementacja programowa nie gwarantuje docelowego FPS podczas panoramowania dużych widoków. Wydajność mierzy `python/tests/benchmark_render.py` (nieruchomy widok, pan i zoom, opcjonalny profil `cProfile`; opcje `--frames`, `--mode`, `--zoom`, `--profile`).

## Edytor plansz
Edytor jest osobną aplikacją (`python/editor.py`, klasy `Editor` i `EditorScene`, funkcje `trim_map()` i `pad_map()`), dzielącą z grą kod rysowania oraz `Board.pick_tile()` (tryb „płaski” wywoływany klawiszem Alt). Klawisze, walidację i operacje na rozmiarze planszy opisuje [specification_of_map_editor.md](specification_of_map_editor.md).

## Format pliku planszy
Format z [specification_of_map_format.md](specification_of_map_format.md) obsługuje `python/hexfront/mapfile.py`: tabele `BUILDING_CODES` i `OBSTACLE_CODES` oraz stałe `BRIDGE_CODE_BASE`, `RAMP_CODE_BASE`, `OWNER_CODE_*` i `MAX_SAVED_UNITS`. Rozszerzenie pliku i katalog plansz wyznaczają stałe `MAP_EXTENSION` i `MAPS_DIR` z `python/hexfront/constants.py`, a zgodność kodu z formatem pilnuje test `python/tests/test_mapfile.py` (uruchamiany jako `cd python && python3 -m tests.test_mapfile`).
