# AGENTS.md — instrukcje dla AI rozwijającego grę War Regions

Ten plik zawiera wyłącznie instrukcje pracy dla asystenta AI. Nie powiela zasad gry ani specyfikacji — wskazuje dokumenty źródłowe jako jedyne źródło prawdy.

## 1. Dokumenty źródłowe (czytaj przed każdą zmianą)

| Dokument | Co zawiera | Kiedy czytać |
|---|---|---|
| `rules.md` | Zasady gry (plansza, budynki, pojazdy, walka, działka, wieże, AI). Jednostki odległości (j). | Zawsze przed zmianą logiki gry. Nie kopiuj stąd liczb do innych plików — odwołuj się linkiem. |
| `specification.md` | Specyfikacja implementacji: kod, grafika, sterowanie, parametry (1 j = 1 px, bok hexu, FPS), format pliku planszy. | Przed zmianą renderingu, sterowania, formatu map. |
| `specification_of_map_editor.md` | Uzupełnienie specyfikacji o edytor map (klawisze, walidacja, trim/pad). | Przed zmianą `editor.py` i `war_regions/mapfile.py`. |
| `README.md` | Skrócony opis uruchomienia i sterowania dla gracza. | Przy zmianie UX / dodawaniu poziomu. |

Zasada: nie przepisuj liczb ani reguł z `rules.md` / `specification*.md` do kodu ani do tego pliku. W kodzie używaj stałych z `war_regions/constants.py`; w dokumentacji dawaj odnośniki do sekcji źródłowych.

Język: dokumenty (`rules.md`, `specification*.md`, ten plik) są po polsku. Kod, komentarze i docstringi są po angielsku — nowe funkcje/metody/klasy/pola też pisz po angielsku z dokumentacją.

## 2. Układ repozytorium

```text
main.py                 punkt wejścia gry (python3 main.py)
editor.py               punkt wejścia edytora map (python3 editor.py [mapa])
make_maps.py            regeneruje przykładowe mapy z generatora do maps/
war_regions/            pakiet gry (logika + UI)
maps/*.map              pliki binarne plansz (nazwa pliku = nazwa poziomu w menu)
tests/                  testy headless (dummy video driver)
```

Menu gry listuje dynamicznie wszystkie `maps/*.map` przez `mapfile.list_maps()` — nie hardkoduj listy poziomów.

## 3. Moduły Pythona (krótki opis — szczegóły w docstringach)

### Root
- `main.py` — tylko `Application().run()`. Nic tu nie dopisuj.
- `editor.py` — osobna aplikacja edytora. Klasy `Editor`, `EditorScene`; funkcje `trim_map()`, `pad_map()`. Dzieli kod rysowania i `pick_tile` z grą. Klawisze w docstringu pliku i w `specification_of_map_editor.md`.
- `make_maps.py` — `main()`: buduje poziomy z `war_regions/levels.py: LEVELS` przez `build_level()` i zapisuje przez `mapfile.save_map()` do `maps/`.

### Pakiet `war_regions/`
- `war_regions/__init__.py` — pusty znacznik pakietu.
- `war_regions/constants.py` — musi zawierać wszystkie potrzebne stałe zdefiniowane przez zasady (`rules.md`) i specyfikację (`specification.md`, `specification_of_map_editor.md`) i być z nimi zgodny (jednostki j, prędkości, zasięgi, obrażenia, kolory, limity edytora, presety `AIDifficulty` / `AI_DIFFICULTIES`). Przelicznik `UNIT_J_TO_PX` dokładnie raz. Każdą nową liczbę dopisz tutaj z komentarzem, do której sekcji `rules.md` się odnosi.
- `war_regions/hexgrid.py` — czysta geometria flat-top hex (odd-q): `neighbor()`, `neighbors()`, `hex_to_world()`, `world_to_hex()`, `hex_distance()`, `hex_corners()`, `edge_dir_index()`. Bez logiki gry.
- `war_regions/board.py` — plansza: `Tile`, `Obstacle` (kindy: `wall`, `mine`, `mine_water`, `trap_fire`, `trap_ice`), `Bridge`, `Board` (`passable()`, `find_path()` BFS ignorujący miny/pułapki/ściany, `reachable()`, `pick_tile()` z trybem `flat` na klawisz Alt, `set_ramp()` / `remove_ramp()`, `add_bridge()`).
- `war_regions/entities.py` — dane: `BuildingKind` (4 bazy, 3 działka, wieża), `Player`, `Building`, `Vehicle`; helpery `is_base()`, `is_turret()`, `vehicle_kind_of()`, `turret_kind_of()`, `capacity_of()`. Logika w `game.py`.
- `war_regions/game.py` — symulacja czasu rzeczywistego: `Game` (`try_send()`, `update()` o stałym kroku `SIM_DT`, produkcja baz, przeludnienie, działka, wieże, aura bufora, ruch/walka/pułapki/miny/ściany, przejęcia budynków, eliminacja/zwycięstwo). Nic o pygame.
- `war_regions/ai.py` — `AIController` (pętla decyzyjna z sekcji 13 `rules.md`: scoring `W1–W6`, próg, szum, opóźnienie reakcji, reguły bezpieczeństwa). Determinystyczne dla danego seeda.
- `war_regions/levels.py` — generator proceduralny: `LevelConfig`, `LEVELS`, `build_level()`. Używany przez `make_maps.py` i testy.
- `war_regions/mapfile.py` — format binarny `.map` (nagłówek, 4-bitowe wysokości, rekordy obiektów, kody `BUILDING_CODES` / `OBSTACLE_CODES` / osie mostów i podjazdów): `save_map()`, `load_board()`, `load_game()`, `list_maps()`, `level_seed()`, `rebuild_bridges()`. Jedynie tu wolno ruszać format pliku.
- `war_regions/camera.py` — rzut izometryczny i widok: `Camera` (`world_to_screen()`, `screen_to_world()`, `pan()`, `zoom_at()`, `center_on_world()`, `limit_to_board()`). Zoom dotyczy tylko renderingu.
- `war_regions/render.py` — rysowanie z kodu (bez assetów rastrowych): `Renderer.draw_world()` + helpery terenu, zasięgów, dróg, obiektów, badge z liczbą jednostek, pływające `-x/+x`. Kolor gracza zawsze jako argument.
- `war_regions/app.py` — okno, menu poziomów, input i HUD: `Application` (`run()`, obsługa LMB/RMB/Esc/P, drag/strzałki/WASD/krawędź, kółko/`+`/`-`, podgląd trasy, pauza). Wybór poziomu kliknięciem; Esc wraca do menu.

### Testy `tests/`
- `tests/test_logic.py` — reguły headless (`python3 -m tests.test_logic`): ruch, produkcja, walki, działka, leczenie, ściany/miny/pułapki, eliminacja, AI, pełne symulacje poziomów.
- `tests/test_mapfile.py` — format map i edytor (`python3 -m tests.test_mapfile`): round-trip, bity właściciela/jednostek, `load_game`, `trim_map`/`pad_map`, akcje i błędy edytora, `pick_tile(flat=True)`.
- `tests/test_render.py` — regresja renderingu (`python3 -m tests.test_render`, wymaga `numpy`): czyszczenie widoku przy pan/zoom, culling kafelków.

## 4. Zasady pracy AI

1. Przed kodem przeczytaj właściwy dokument z sekcji 1 i docstringi edytowanego modułu. Nie zgaduj wartości — sprawdź `constants.py` i wskazaną sekcję `rules.md`.
2. Trzymaj separację: logika w `game.py` / `board.py` / `ai.py`, rysowanie w `render.py`, input w `app.py` / `editor.py`. Wyjątki od reguły tylko z uzasadnieniem w komentarzu.
3. Nowe liczby tylko do `constants.py` z komentarzem dokumentującym (która sekcja zasad). Nigdy nie hardkoduj zasięgów/prędkości/obrażeń w logice ani w rendererze.
4. Format `.map` zmieniasz wyłącznie w `mapfile.py` (i testach w `test_mapfile.py`) i pilnuj zgodności z jego opisem w `specification.md`
5. Edytor i gra współdzielą `Renderer` i `Board.pick_tile()` — nie rozjeżdżaj ich (pick z Alt jako `flat=True`).
6. Zachowaj konwencje: angielskie identyfikatory i docstringi, czytelne funkcje, pełne sygnatury bez placeholderów.
7. Uruchamianie: `python3 main.py`, `python3 editor.py`, `python3 make_maps.py` (wymaga `pygame`; testy renderujące także `numpy`).
8. Po każdej zmianie logiki/formatu uruchom odpowiadające testy headless (`python3 -m tests.test_logic`, `python3 -m tests.test_mapfile`, przy zmianach graficznych także `python3 -m tests.test_render`) i dopisz test przy nowej regule.

## 5. Obowiązek aktualizacji tego pliku

Jeśli dodasz lub usuniesz plik `.py` / katalog, albo istotnie zmienisz odpowiedzialność istniejącego modułu (nowa klasa, nowy podsystem, zmiana formatu map, nowy dokument `.md` ze źródłem prawdy), zaktualizuj w tym samym commicie sekcje 2–3 powyżej: dopisz/usuń wpis z pełną ścieżką i jednozdaniowym opisem roli. Nie dopisuj tu liczb ani reguł gry — tylko strukturę kodu i odnośniki.

