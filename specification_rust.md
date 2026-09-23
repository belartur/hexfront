# Hexfront — specyfikacja implementacji w Rust

## Wstęp
Ta specyfikacja opisuje implementację gry w Rust. Implementacja istnieje i jest kompletna (gra + testy w `rust/`); edytor plansz pozostaje poza jej zakresem. Plik [specification.md](specification.md) opisuje część implementacji niezależną od języka. Zasady gry pozostają jedynym źródłem prawdy w [rules.md](rules.md). Nie zakładamy zgodności wewnętrznej z implementacją Pythonową ([specification_python.md](specification_python.md)) — wiążą nas wspólny kontrakt z [specification.md](specification.md) i format planszy z [specification_of_map_format.md](specification_of_map_format.md), a podział modułów, nazwy i rozwiązania techniczne mogą być inne.

Edytor plansz jest poza zakresem tej implementacji — istnieje tylko w wersji Pythonowej ([specification_of_map_editor.md](specification_of_map_editor.md)). Poza zakresem jest też generator poziomów: pliki `maps/*.map` są wersjonowane, a regeneruje je narzędzie Pythona (`python/make_maps.py`); implementacja Rust jedynie je czyta.

Decyzje zapisane w tej specyfikacji (nazwy modułów, nazwy stałych, rozwiązania techniczne) można zmieniać przy implementacji, o ile jest ku temu konkretny powód — zawsze wraz z aktualizacją tej specyfikacji w tym samym commicie (zasada wspólna z [specification.md](specification.md), sekcja „Kod”).

## Język i biblioteki
* stabilny Rust z systemu (rustc/cargo, obecnie 1.98.1), edycja 2024,
* crate binarny o nazwie `hexfront`,
* grafika, okno i obsługa wejścia: biblioteka **macroquad** (wersja z gałęzi 0.4, obecnie 0.4.16) pobierana z crates.io i pinowana w `Cargo.toml`,
* brak innych zależności na start; każdy dodatkowy crate wymaga uzasadnienia w komentarzu w `Cargo.toml` (np. generator liczb losowych do szumu AI, gdyby własny generator w `rng.rs` okazał się niewystarczający),
* moduły logiki gry (`hexgrid`, `board`, `entities`, `game`, `ai`, `rng`, `mapfile`) nie zależą od macroquad — dzięki temu testy logiki działają bez okna.

## Planowany układ kodu
```text
rust/Cargo.toml        manifest crate'a (Cargo.lock wersjonowany; target/ w .gitignore)
rust/src/main.rs       punkt wejścia gry (cargo run --release)
rust/src/constants.rs  stałe gry: UNIT_J_TO_PX jako jedyne miejsce przelicznika j → px, FPS i SIM_DT (prędkość klatki — decyzja tej implementacji), współczynniki rzutu izometrycznego, REPO_ROOT, MAPS_DIR, MAP_EXTENSION; każda stała z komentarzem wskazującym sekcję rules.md lub specification.md
rust/src/hexgrid.rs    geometria sześciokątów flat-top (odd-q) bez logiki gry
rust/src/board.rs      plansza: pola, utrudnienia, podjazdy, mosty, wyszukiwanie drogi, wskazywanie pola kursorem (tryb „płaski” z klawiszem Alt)
rust/src/entities.rs   rodzaje budynków, gracze, budynki, pojazdy i ich pojemności
rust/src/game.rs       symulacja czasu rzeczywistego o stałym kroku SIM_DT, bez zależności od macroquad
rust/src/ai.rs         sterowanie przeciwnikami według sekcji 13 rules.md; deterministyczne dla danego seeda
rust/src/rng.rs        własny deterministyczny PRNG (bez dodatkowych crate'ów, niezależny od macroquad)
rust/src/mapfile.rs    zapis i odczyt formatu z specification_of_map_format.md (tabele kodów) oraz lista poziomów dla menu
rust/src/camera.rs     rzut izometryczny i widok: przesuwanie, zoom, ograniczanie do planszy
rust/src/iso.rs        ortograficzna kamera GPU odtwarzająca rzut 2D (macierze view/proj, głębia z D wzdłuż osi Z)
rust/src/mesh.rs       budowa meshy GPU bez zależności od macroquad: statyczny teren (chunki na u16) i dynamiczne obiekty co klatkę
rust/src/render.rs     rysowanie z kodu na GPU, bez assetów rastrowych (chunki terenu, linie siatki, obiekty, przezroczyste zasięgi, kreski 3D, nakładki 2D)
rust/src/render_baseline.rs  test-benchmark budowy meshy (`#[cfg(test)]`, tylko na potrzeby pomiaru)
rust/src/app.rs        okno, menu poziomów, obsługa wejścia i pętla gry
```

## Uruchamianie i testy
```bash
cd rust && cargo run --release   # gra
cd rust && cargo test            # testy
```

Kod formatowany jest rustfmem (domyślne ustawienia). Testy logiki działają bez okna, bo moduły logiki nie zależą od macroquad; są to moduły `#[cfg(test)]` wewnątrz modułów logiki. Zakres: reguły z [rules.md](rules.md) (ruch, produkcja, walka, działka, wieże, eliminacja, AI) oraz format plansz — odczyt tych samych plików z `maps/` musi dawać ten sam stan początkowy w każdej implementacji, co wynika z formatu ([specification_of_map_format.md](specification_of_map_format.md)), a nie z jednej implementacji; test formatu czyta rzeczywiste pliki z `maps/`.

## Renderowanie
Wspólny kontrakt ([specification.md](specification.md), sekcja „Grafika i interfejs użytkownika”) wymaga bufora głębokości z D = (x + y)·k + z. Scenę budujemy z meshy trójkątów w pamięci CPU (`mesh.rs`, bez zależności od macroquad) i rysujemy je standardowym pipelinem macroquad z włączonym sprzętowym testem głębokości (`LessOrEqual`, zapis głębokości dla nieprzezroczystych): ortograficzna kamera GPU (`iso.rs`) odtwarza rzut 2D z `camera.rs` (pan/zoom to tylko zmiana macierzy — zero pracy CPU na klatkę), a oś głębokości niesie D, więc sprzęt rozstrzyga zasłanianie jak dawny bufor programowy (większe D wygrywa, remisy zachowują kolejność przejść). Tekst interfejsu (menu, liczniki jednostek, pływające -x/+x) rysujemy na wierzchu wbudowanym fontem macroquad.

Kolejność przejść: nieprzezroczyste chunki terenu (wierzchy pól, skarpy, rampy, mosty) → linie siatki terenu → nieprzezroczyste obiekty dynamiczne (budynki, utrudnienia, pojazdy, pociski) → przezroczyste wypełnienia zasięgów (bez zapisu głębokości) → kreski 3D (obrysy zasięgów, drogi, detale) → nakładki 2D (obrys zaznaczenia, podgląd trasy) → teksty. Cieni programowych nie ma (GPU nie potrzebuje bufora cieni); wygląd celowo nie musi być zgodny piksel-w-piksel z implementacją Pythonową — wiąże kontrakt, nie piksele.

Teren statyczny budujemy raz na poziom (`build_terrain()`; chunki do 60 000 wierzchołków, bo macroquad batchuje draw calle w buforach indeksów u16), obiekty dynamiczne co klatkę (`build_dynamic()`). Przebudowa terenu następuje tylko przy zmianie planszy. Culling widoku realizuje sprzęt (clipping GPU); jawnego odrzucania prymitywów przed projekcją nie ma. Pomiar kosztu budowy meshy daje `rust/src/render_baseline.rs` (test `render_baseline`, `cargo test --release render_baseline -- --nocapture`). Twardych wymagań czasowych nie stawia żadna specyfikacja — pętlę klatki steruje macroquad (`next_frame`).

## Struktura aplikacji
Aplikacja ma dwa stany: menu poziomów i gra; pauza jest flagą stanu gry. Okno tworzymy w domyślnej konfiguracji macroquad — nie wpisujemy rozmiaru, domyślne okno jest skalowalne przez użytkownika, a wymiar renderowania pobieramy co klatkę z `screen_width()`/`screen_height()`. Zmiana rozmiaru okna wymusza przebudowanie buforów rasteryzacji i unieważnia cache terenu. Obsługa wejścia — przesuwanie, zoom, zaznaczanie i wysyłanie pojazdów, tryb „płaski” z klawiszem Alt, pauza i semantyka Esc — jest opisana w [specification.md](specification.md) (sekcja „Sterowanie”) i obowiązuje tu bez zmian.

## Determinizm i czas
Prędkość klatki jest decyzją tej implementacji: `FPS` i `SIM_DT` to stałe w `constants.rs`, a symulacja działa o stałym kroku `SIM_DT` niezależnie od zegara systemowego — czas rzeczywisty służy wyłącznie do zliczania kroków. Symulacja i AI są deterministyczne dla danego seeda (sekcja 13 [rules.md](rules.md)); seed wyznaczany jest z nazwy pliku poziomu, jak w implementacji Pythonowej. Determinizm między implementacjami nie jest wymagany — wymagany jest determinizm wewnątrz tej implementacji. Losowość (szum AI) realizuje `rng.rs` własnym, deterministycznym generatorem, bez dodatkowych crate'ów.

## Katalog maps
Ścieżkę katalogu `maps` wyznaczamy względem katalogu głównego repozytorium, a nie względem katalogu roboczego — to zasada z [specification.md](specification.md) (sekcja „Struktura repozytorium”). Punktem wyjścia jest katalog nadrzędny wobec `env!("CARGO_MANIFEST_DIR")`; przy uruchomieniu z gotowej binarki sprawdzamy kolejne katalogi nadrzędne wobec katalogu binarki. Menu poziomów listuje wszystkie pliki `*.map` z tego katalogu (nazwa pliku jest nazwą poziomu), bez hardkodowania listy.

