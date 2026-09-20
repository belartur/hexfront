# AGENTS.md — instrukcje dla AI rozwijającego grę Hexfront

Ten plik zawiera wyłącznie instrukcje pracy dla asystenta AI. Nie powiela zasad gry ani specyfikacji — wskazuje dokumenty źródłowe jako jedyne źródło prawdy.

## 1. Dokumenty źródłowe (czytaj przed każdą zmianą)

| Dokument | Co zawiera | Kiedy czytać |
|---|---|---|
| `rules.md` | Zasady gry (plansza, budynki, pojazdy, walka, działka, wieże, AI). Jednostki odległości (j). | Zawsze przed zmianą logiki gry. Nie kopiuj stąd liczb do innych plików — odwołuj się linkiem. |
| `specification.md` | Część wspólna specyfikacji implementacji: struktura repozytorium, kod, kontrakt wizualny, sterowanie, parametry (1 j = 1 px, bok hexu, FPS), plansze. | Przed zmianą wyglądu, sterowania lub parametrów wspólnych dla wszystkich implementacji. |
| `specification_python.md` | Specyfikacja implementacji Python (PyGame + NumPy): układ kodu, uruchamianie i testy, mechanika renderera (NumPy, cache), edytor, format mapy. | Przed zmianą `python/**` (poza czystą logiką gry, którą opisuje `rules.md`). |
| `specification_rust.md` | Specyfikacja implementacji Rust (stabilny rustc/cargo + macroquad) — szkielet planu, kod jeszcze nie istnieje. | Przed rozpoczęciem lub rozwojem kodu w `rust/`. |
| `specification_of_map_format.md` | Binarny format pliku planszy: układ bajtów, tabele typów budynków i utrudnień, kodowanie mostów i podjazdów. Wspólny dla wszystkich implementacji (bez odwołań do kodu). | Przed zmianą formatu `.map` oraz `python/hexfront/mapfile.py`. |
| `specification_of_map_editor.md` | Uzupełnienie specyfikacji o edytor map (klawisze, walidacja, trim/pad). | Przed zmianą `python/editor.py` i `python/hexfront/mapfile.py`. |
| `README.md` | Skrócony opis uruchomienia i sterowania dla gracza. | Przy zmianie UX / dodawaniu poziomu. |

Zasada: nie przepisuj liczb ani reguł z `rules.md` / `specification*.md` do kodu ani do tego pliku. W kodzie używaj stałych z `python/hexfront/constants.py`; w dokumentacji dawaj odnośniki do sekcji źródłowych.

Język: dokumenty (`rules.md`, `specification*.md`, ten plik) są po polsku. Kod, komentarze i docstringi są po angielsku — nowe funkcje/metody/klasy/pola też pisz po angielsku z dokumentacją. Commity gita opisuj po angielsku.

## 2. Układ repozytorium

Dokumenty źródłowe (`rules.md`, `specification.md`, `specification_python.md`, `specification_rust.md`, `specification_of_map_format.md`, `specification_of_map_editor.md`) oraz katalog `maps/` z planszami leżą w katalogu głównym repozytorium. Kod każdej implementacji języka ma osobny katalog — obecnie `python/`, docelowo także `rust/`:

```text
python/main.py               punkt wejścia gry (python3 python/main.py)
python/editor.py             punkt wejścia edytora map (python3 python/editor.py [mapa])
python/make_maps.py          regeneruje przykładowe mapy z generatora do maps/
python/hexfront/             pakiet gry (logika + UI)
python/tests/                testy headless (dummy video driver)
maps/*.map                   pliki binarne plansz (nazwa pliku = nazwa poziomu w menu)
rust/                        implementacja w Rust (planowana; szczegóły w specification_rust.md)
```

Menu gry listuje dynamicznie wszystkie `maps/*.map` przez `mapfile.list_maps()` — nie hardkoduj listy poziomów. Ścieżka do katalogu `maps/` liczona jest od katalogu głównego repozytorium (`REPO_ROOT` i `MAPS_DIR` w `python/hexfront/constants.py`), więc grę, edytor i testy można uruchamiać z dowolnego katalogu roboczego. Implementacja w kolejnym języku dostaje własny katalog obok `python/` i nie zmienia `maps/` ani dokumentów źródłowych.

## 3. Moduły implementacji (krótki opis — szczegóły w docstringach)

### Katalog `python/` (punkty wejścia)
- `python/main.py` — tylko `Application().run()`. Nic tu nie dopisuj.
- `python/editor.py` — osobna aplikacja edytora. Klasy `Editor`, `EditorScene`; funkcje `trim_map()`, `pad_map()`. Dzieli kod rysowania i `pick_tile` z grą. Klawisze w docstringu pliku i w `specification_of_map_editor.md`.
- `python/make_maps.py` — `main()`: buduje poziomy z `python/hexfront/levels.py: LEVELS` przez `build_level()` i zapisuje przez `mapfile.save_map()` do `maps/` (ścieżka z `constants.MAPS_DIR`).

### Pakiet `python/hexfront/`
- `python/hexfront/__init__.py` — pusty znacznik pakietu.
- `python/hexfront/constants.py` — musi zawierać wszystkie potrzebne stałe zdefiniowane przez zasady (`rules.md`) i specyfikację (`specification.md`, `specification_of_map_format.md`, `specification_of_map_editor.md`) i być z nimi zgodny (jednostki j, prędkości, zasięgi, obrażenia, kolory, limity edytora, presety `AIDifficulty` / `AI_DIFFICULTIES`). Przelicznik `UNIT_J_TO_PX` dokładnie raz. Każdą nową liczbę dopisz tutaj z komentarzem, do której sekcji `rules.md` się odnosi. Tu też mieszkają ścieżki `REPO_ROOT` i `MAPS_DIR` (katalog `maps/` w katalogu głównym repozytorium).
- `python/hexfront/hexgrid.py` — czysta geometria flat-top hex (odd-q): `neighbor()`, `neighbors()`, `hex_to_world()`, `world_to_hex()`, `hex_distance()`, `hex_corners()`, `edge_dir_index()`. Bez logiki gry.
- `python/hexfront/board.py` — plansza: `Tile`, `Obstacle` (kindy: `wall`, `mine`, `mine_water`, `trap_fire`, `trap_ice`), `Bridge`, `Board` (`passable()`, `find_path()` BFS ignorujący miny/pułapki/ściany, `reachable()`, `pick_tile()` z trybem `flat` na klawisz Alt, `set_ramp()` / `remove_ramp()`, `add_bridge()`).
- `python/hexfront/entities.py` — dane: `BuildingKind` (4 bazy, 3 działka, wieża), `Player`, `Building`, `Vehicle`; helpery `is_base()`, `is_turret()`, `vehicle_kind_of()`, `turret_kind_of()`, `capacity_of()`. Logika w `game.py`.
- `python/hexfront/game.py` — symulacja czasu rzeczywistego: `Game` (`try_send()`, `update()` o stałym kroku `SIM_DT`, produkcja baz, przeludnienie, działka, wieże, aura bufora, ruch/walka/pułapki/miny/ściany, przejęcia budynków, eliminacja/zwycięstwo). Nic o pygame.
- `python/hexfront/ai.py` — `AIController` (pętla decyzyjna z sekcji 13 `rules.md`: scoring `W1–W6`, próg, szum, opóźnienie reakcji, reguły bezpieczeństwa). Determinystyczne dla danego seeda.
- `python/hexfront/levels.py` — generator proceduralny: `LevelConfig`, `LEVELS`, `build_level()`. Używany przez `make_maps.py` i testy.
- `python/hexfront/mapfile.py` — implementacja formatu `.map` opisanego w `specification_of_map_format.md` (tabele `BUILDING_CODES` / `OBSTACLE_CODES`): `save_map()`, `load_board()`, `load_game()`, `list_maps()`, `level_seed()`, `rebuild_bridges()`. Jedynie tu (i w `python/tests/test_mapfile.py`) wolno ruszać format pliku.
- `python/hexfront/camera.py` — rzut izometryczny i widok: `Camera` (`world_to_screen()`, `screen_to_world()`, `pan()`, `zoom_at()`, `center_on_world()`, `limit_to_board()`). Zoom dotyczy tylko renderingu.
- `python/hexfront/depth.py` — programowy bufor głębokości NumPy: `ProjectedPoint`, `DepthCamera`, `DepthBuffer`; rasteryzacja powierzchni i linii sceny, zbiorczy test głębokości poziomych pól i siatki oraz przezroczyste cienie na powierzchniach przyjmujących (bez zapisu głębokości).
- `python/hexfront/render.py` — rysowanie z kodu (bez assetów rastrowych): `Renderer.draw_world()`, cache obrazu/głębokości nieruchomego terenu + helpery terenu, zasięgów, dróg, obiektów, badge z liczbą jednostek, pływające `-x/+x`. Kolor gracza zawsze jako argument.
- `python/hexfront/app.py` — okno, menu poziomów, input i HUD: `Application` (`run()`, obsługa LMB/RMB/Esc/P, drag/strzałki/WASD/krawędź, kółko/`+`/`-`, podgląd trasy, pauza). Wybór poziomu kliknięciem; Esc wraca do menu.

### Testy `python/tests/`
- `python/tests/benchmark_render.py` — powtarzalny benchmark `draw_world` z ruchomymi pojazdami: nieruchomy widok, pan i zoom; opcjonalny profil cProfile.
- `python/tests/test_logic.py` — reguły headless (`cd python && python3 -m tests.test_logic`): ruch, produkcja, walki, działka, leczenie, ściany/miny/pułapki, eliminacja, AI, pełne symulacje poziomów.
- `python/tests/test_mapfile.py` — format map (`specification_of_map_format.md`) i edytor (`cd python && python3 -m tests.test_mapfile`): `load_game`, `trim_map`/`pad_map`, akcje i błędy edytora, `pick_tile(flat=True)`.
- `python/tests/test_render.py` — regresja renderingu (`cd python && python3 -m tests.test_render`, wymaga `numpy`): czyszczenie widoku przy pan/zoom, culling, widoczność pojazdów i ramp, zasłanianie mostów, bufor głębokości i unieważnianie cache.

### Katalog `rust/` (implementacja w Rust — planowana)
- Kodu jeszcze nie ma. Plan układu, zakres i otwarte punkty opisuje `specification_rust.md` (stabilny Rust + macroquad, moduły 1:1 z `python/hexfront/`, edytor poza zakresem, `Cargo.lock` wersjonowany, `target/` ignorowany).
- Po dodaniu crate'a dopisz tutaj opis modułów `rust/src/` i zaktualizuj sekcję 2 (drzewo repozytorium).

## 4. Zasady pracy AI

1. Przed kodem przeczytaj właściwy dokument z sekcji 1 i docstringi edytowanego modułu. Nie zgaduj wartości — sprawdź `constants.py` i wskazaną sekcję `rules.md`.
2. Trzymaj separację: logika w `game.py` / `board.py` / `ai.py`, rysowanie w `render.py`, input w `app.py` / `editor.py`. Wyjątki od reguły tylko z uzasadnieniem w komentarzu.
3. Nowe liczby tylko do `constants.py` z komentarzem dokumentującym (która sekcja zasad). Nigdy nie hardkoduj zasięgów/prędkości/obrażeń w logice ani w rendererze.
4. Opis formatu `.map` jest w `specification_of_map_format.md` (dokument wspólny dla wszystkich implementacji, bez odwołań do kodu); implementację zmieniasz wyłącznie w `python/hexfront/mapfile.py` (i testach w `python/tests/test_mapfile.py`). Tabele kodów w `mapfile.py` są implementacją tabel z tej specyfikacji — zmieniając format, zaktualizuj oba miejsca. Wszystkie ścieżki plików licz od `REPO_ROOT` / `MAPS_DIR` z `constants.py`, nie od katalogu roboczego.
5. Edytor i gra współdzielą `Renderer` i `Board.pick_tile()` — nie rozjeżdżaj ich (pick z Alt jako `flat=True`).
6. Zachowaj konwencje: angielskie identyfikatory i docstringi, czytelne funkcje, pełne sygnatury bez placeholderów.
7. Uruchamianie (z dowolnego katalogu, ścieżki do map są liczone od roota repozytorium): `python3 python/main.py`, `python3 python/editor.py`, `python3 python/make_maps.py` (gra, edytor i testy renderujące wymagają `pygame` oraz `numpy`). Testy uruchamia się z katalogu `python/`: `cd python && python3 -m tests.test_logic`.
8. Po każdej zmianie logiki/formatu uruchom odpowiadające testy headless (`cd python && python3 -m tests.test_logic`, `cd python && python3 -m tests.test_mapfile`, przy zmianach graficznych także `cd python && python3 -m tests.test_render`) i dopisz test przy nowej regule.
9. Respektuj `.gitignore` — to on jest źródłem prawdy, co commitować; nie dodawaj na siłę plików ignorowanych. Pliki `maps/*.map` są wersjonowane; jeśli brakuje ich po świeżym klonie, odtwórz je poleceniem `python3 python/make_maps.py`.
10. Pilnuj zgodności kodu ze specyfikacją: jeśli zadanie zmienia zachowanie, parametr lub format opisany w `specification.md` (część wspólna) / `specification_python.md` / `specification_rust.md` / `specification_of_map_format.md` / `specification_of_map_editor.md` / `rules.md`, zaktualizuj w tym samym commicie i kod, i odpowiedni dokument, żeby pozostały zgodne. Zmiana kontraktu wspólnego (wygląd, sterowanie, parametry) wymaga edycji `specification.md`, a nie tylko specyfikacji języka; format mapy zmieniaj tylko wraz z `specification_of_map_format.md` (w tym pliku nie umieszczaj odwołań do kodu żadnej implementacji).

## 5. Obowiązek aktualizacji tego pliku

Jeśli dodasz lub usuniesz plik `.py` / katalog, albo istotnie zmienisz odpowiedzialność istniejącego modułu (nowa klasa, nowy podsystem, zmiana formatu map, nowy dokument `.md` ze źródłem prawdy), zaktualizuj w tym samym commicie sekcje 1–3 powyżej: dopisz/usuń wpis (tabela dokumentów źródłowych, układ repozytorium, opis modułu) z pełną ścieżką i jednozdaniowym opisem roli. Nie dopisuj tu liczb ani reguł gry — tylko strukturę kodu i odnośniki.

