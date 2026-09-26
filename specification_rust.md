# Hexfront — specyfikacja implementacji w Rust

## Wstęp
Ta specyfikacja opisuje implementację gry w Rust. Implementacja gry istnieje i jest kompletna (gra + testy w `rust/`); edytor plansz jest planowany jako integralna część tej samej aplikacji (stan edytora, nie osobny program) i opisuje go sekcja „Edytor plansz” w tym pliku. Plik [specification.md](specification.md) opisuje część implementacji niezależną od języka. Zasady gry pozostają jedynym źródłem prawdy w [rules.md](rules.md). Nie zakładamy zgodności wewnętrznej z implementacją Pythonową ([specification_python.md](specification_python.md)) — wiążą nas wspólny kontrakt z [specification.md](specification.md) i format planszy z [specification_of_map_format.md](specification_of_map_format.md), a podział modułów, nazwy i rozwiązania techniczne mogą być inne.

Szczegóły edytora Pythonowego opisuje [specification_of_map_editor.md](specification_of_map_editor.md); edytor Rustowy kopiuje stamtąd zachowanie (klawisze, walidację, operacje trim/pad), ale jest wbudowany w grę, a nie osobną aplikacją — dlatego jego pełna specyfikacja jest w tym pliku, a nie w osobnym dokumencie. Poza zakresem jest generator poziomów: pliki `maps/*.map` są wersjonowane, a regeneruje je narzędzie Pythona (`python/make_maps.py`); implementacja Rust jedynie je czyta i zapisuje.

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
rust/src/mapfile.rs    zapis i odczyt formatu z specification_of_map_format.md (tabele kodów) oraz lista poziomów dla menu; używany też przez edytor
rust/src/camera.rs     rzut izometryczny i widok: przesuwanie, zoom, ograniczanie do planszy
rust/src/iso.rs        ortograficzna kamera GPU odtwarzająca rzut 2D (macierze view/proj, głębia z D wzdłuż osi Z)
rust/src/mesh.rs       budowa meshy GPU bez zależności od macroquad: statyczny teren (chunki na u16) i dynamiczne obiekty co klatkę
rust/src/render.rs     rysowanie z kodu na GPU, bez assetów rastrowych (chunki terenu, linie siatki, obiekty, przezroczyste zasięgi, kreski 3D, nakładki 2D)
rust/src/render_baseline.rs  test-benchmark budowy meshy (`#[cfg(test)]`, tylko na potrzeby pomiaru)
rust/src/editor.rs     planowany edytor map jako stan aplikacji (nie osobny program): operacje na `Board`, trim/pad, walidacja, pamięć ostatniego budynku/utrudnienia
rust/src/app.rs        okno, menu poziomów, gra i edytor: obsługa wejścia i pętla aplikacji
```

## Uruchamianie i testy
```bash
cd rust && cargo run --release   # gra
cd rust && cargo test            # testy
```

Kod formatowany jest rustfmtem (domyślne ustawienia). Testy logiki działają bez okna, bo moduły logiki nie zależą od macroquad; są to moduły `#[cfg(test)]` wewnątrz modułów logiki. Zakres: reguły z [rules.md](rules.md) (ruch, produkcja, walka, działka, wieże, eliminacja, AI), edytor (trim/pad, walidacja, cykle klawiszy — headless, bez okna) oraz format plansz — odczyt tych samych plików z `maps/` musi dawać ten sam stan początkowy w każdej implementacji, co wynika z formatu ([specification_of_map_format.md](specification_of_map_format.md)), a nie z jednej implementacji; test formatu czyta rzeczywiste pliki z `maps/`.

## Renderowanie
Wspólny kontrakt ([specification.md](specification.md), sekcja „Grafika i interfejs użytkownika”) wymaga bufora głębokości z D = (x + y)·k + z. Scenę budujemy z meshy trójkątów w pamięci CPU (`mesh.rs`, bez zależności od macroquad) i rysujemy je standardowym pipelinem macroquad z włączonym sprzętowym testem głębokości (`LessOrEqual`, zapis głębokości dla nieprzezroczystych): ortograficzna kamera GPU (`iso.rs`) odtwarza rzut 2D z `camera.rs` (pan/zoom to tylko zmiana macierzy — zero pracy CPU na klatkę), a oś głębokości niesie D, więc sprzęt rozstrzyga zasłanianie jak dawny bufor programowy (większe D wygrywa, remisy zachowują kolejność przejść). Tekst interfejsu (menu, liczniki jednostek, pływające -x/+x) rysujemy na wierzchu wbudowanym fontem macroquad.

Kolejność przejść: nieprzezroczyste chunki terenu (wierzchy pól, skarpy, rampy, mosty) → linie siatki terenu → nieprzezroczyste obiekty dynamiczne (budynki, utrudnienia, pojazdy, pociski) → przezroczyste cienie helikopterów → przezroczyste wypełnienia zasięgów (bez zapisu głębokości) → kreski 3D (obrysy zasięgów, drogi, detale) → nakładki 2D (obrys zaznaczenia, podgląd trasy) → teksty. Cień helikoptera to półprzezroczysta **sylwetka** rzutowana prosto w dół (a nie pojedynczy dysk o promieniu 14 j z [specification.md](specification.md), którego rozwiązanie zachowuje tylko same wartości koloru/alfy): obracające się łopaty wirnika, płozy, belka ogona ze statecznikiem i kadłub — wszystkie części płaskie, czarne, alfa 70/255 (kolor i alfa z sekcji „Grafika i interfejs użytkownika” [specification.md](specification.md)), o rozmiarach z tych samych stałych w px co bryły w `mesh.rs`. Sylwetka leży w jednej płaszczyźnie podniesionej o `SHADOW_LIFT` (1 j) nad powierzchnią przyjmującą (nad pokładem mostu ląduje na pokładzie): dość wysoko, żeby nie walczyć z terenem o głębokość (brak migotania), dość nisko, żeby czytała się jako leżąca na ziemi. Jedna płaszczyzna i jedna alfa oznaczają też brak nakładających się przyciemnień w samej sylwetce; sprzętowy test głębokości zastępuje programowe przycinanie cienia z implementacji Pythonowej (nie przyciemnia kadłuba, budynków ani bliższych skarp). Animację wirnika napędza jedna faza (`Renderer::rotor_phase`) wspólna dla łopat bryły i łopat cienia — helikopter i jego cień kręcą się więc synchronicznie — przesuwana o `ROTOR_SPIN_RAD_PER_S` (rad/s, `constants.rs`) na podstawie czasu rzeczywistego klatki, dzięki czemu prędkość obrotu nie zależy od liczby klatek na sekundę; wirnik ogonowy używa tej samej fazy z mnożnikiem `HELI_TAIL_ROTOR_RATIO` z `mesh.rs`. Nie ma osobnego, programowego przebiegu cieni ani bufora cieni; wygląd celowo nie musi być zgodny piksel-w-piksel z implementacją Pythonową — wiąże kontrakt, nie piksele.

Teren statyczny budujemy raz na poziom (`build_terrain()`; chunki do 60 000 wierzchołków, bo macroquad batchuje draw calle w buforach indeksów u16), obiekty dynamiczne co klatkę (`build_dynamic()`). Przebudowa terenu następuje tylko przy zmianie planszy (w edytorze: przy każdej zmianie terenu, rampy lub mostu). Culling widoku realizuje sprzęt (clipping GPU); jawnego odrzucania prymitywów przed projekcją nie ma. Pomiar kosztu budowy meshy daje `rust/src/render_baseline.rs` (test `render_baseline`, `cargo test --release render_baseline -- --nocapture`). Twardych wymagań czasowych nie stawia żadna specyfikacja — pętlę klatki steruje macroquad (`next_frame`).

Pojazdy rysujemy jako bryły zorientowane w świecie, a nie jako prostopadłościany wyrównane do osi: kadłub obraca się wzdłuż kierunku jazdy (jak kadłub i wirnik helikoptera z [`mesh.rs`](rust/src/mesh.rs)), natomiast wieża czołgu obraca się niezależnie i celuje w aktualny cel ostrzału — w pojazd, z którym toczy się walka (sekcja 9 [rules.md](rules.md)), a gdy walki nie ma, w ścianę, którą pojazd ostrzeliwuje (sekcja 4); bez celu lufa spoczywa wzdłuż kierunku jazdy. Bufor używa tego samego kadłuba, ale zamiast działa nosi zielony krzyż leczący (sekcja 5.4). Detale brył (gąsienice z bieżnikiem, pochylony pancerz czołowy, wieża z kopułką dowódcy i koszem tylnym, antena, lufa z hamulcem wylotowym) mają stałe rozmiarów w px zapisane przy kodzie budowy meshy w `mesh.rs`, obok stałych chunka terenu — są to wielkości wyłącznie renderingu (jak grubość linii siatki), niezależne od parametrów gry z `rules.md`. Helikopter (sekcja 5.2 [rules.md](rules.md)) ma smukły, podłużny kadłub z oszkloną kabiną w kolorze przyciemnionej szyby (nie białe pudełko), belkę ogonową ze statecznikiem i wirnikami; leci na **stałej** wysokości nad najwyższym terenem planszy (`HELICOPTER_ALTITUDE_PX`, wielkość wyłącznie renderingu), więc nie skacze po wzgórzach i nigdy nie chowa się za szczytem — dlatego każdy helikopter rysuje ciemny cień na polu pod sobą (patrz wyżej), a jego licznik jednostek i pływające liczby wiszą przy kadłubie, nie na gruncie. Pojazdy naziemne stoją na powierzchni, więc cienia nie dostają.

## Struktura aplikacji
Aplikacja ma trzy stany: menu poziomów, gra i (planowany) edytor plansz; pauza jest flagą stanu gry (w edytorze pauzy nie ma). Okno tworzymy w domyślnej konfiguracji macroquad — nie wpisujemy rozmiaru, domyślne okno jest skalowalne przez użytkownika, a wymiar renderowania pobieramy co klatkę z `screen_width()`/`screen_height()`. Zmiana rozmiaru okna wymusza przebudowanie buforów rasteryzacji i unieważnia cache terenu. Obsługa wejścia — przesuwanie, zoom, zaznaczanie i wysyłanie pojazdów, tryb „płaski” z klawiszem Alt, pauza i semantyka Esc — jest opisana w [specification.md](specification.md) (sekcja „Sterowanie”) i obowiązuje w grze bez zmian. Wejścia do edytora opisuje sekcja „Edytor plansz” poniżej: przycisk `add map` w menu głównym tworzy nową mapę, a kliknięcie pozycji mapy prawym przyciskiem myszy otwiera ją do edycji (kliknięcie lewym przyciskiem bez zmian rozpoczyna grę).

## Edytor plansz (planowany)
Edytor jest integralną częścią tej samej binarki — stan edytora w `app.rs` z logiką w `editor.rs`, a nie osobny program. Zachowanie edycji jest kopią edytora Pythonowego ([specification_of_map_editor.md](specification_of_map_editor.md)): wskazywanie pola myszą identycznie jak w grze (w tym tryb „płaski” z klawiszem Alt przez `Board::pick_tile()` z `flat=true`), ten sam zestaw klawiszy i walidacja, te same operacje trim/pad i ten sam format pliku ([specification_of_map_format.md](specification_of_map_format.md), implementacja w `mapfile.rs`).
Gra i edytor współdzielą renderer (`render.rs`, `mesh.rs`: `TerrainMesh`/`DynamicMesh`) — `Renderer::pick_tile()` / `Renderer::snap_to_building()` tylko delegują do `Board`, jak w grze.

### Wejście i wyjście
* Menu główne pokazuje obok siatki poziomów dodatkowy przycisk `add map` (zawsze widoczny): kliknięcie go lewym przyciskiem myszy otwiera edytor z nową mapą (plansza 256 na 256 — woda z prostokątem 20 na 13 o wysokości 1 na środku, widok wycentrowany; jak w [specification_of_map_editor.md](specification_of_map_editor.md)).
* Kliknięcie pozycji mapy w menu głównym prawym przyciskiem myszy otwiera tę mapę do edycji (z dopełnieniem `pad` do 256 na 256 i wycentrowanym widokiem, jak w [specification_of_map_editor.md](specification_of_map_editor.md)). Kliknięcie lewym przyciskiem bez zmian rozpoczyna grę.
* Prawy przycisk ma więc różne znaczenie w każdym stanie, a stany są rozłączne: w menu — edytuj mapę, w grze — anulowanie zaznaczenia / wybór (sekcja „Sterowanie” [specification.md](specification.md)), w edytorze — kasowanie obiektu (patrz niżej).
* Wyjście z edytora klawiszem Esc wraca do menu głównego; jeśli zmiany nie są zapisane, edytor pyta o zapis (jak w wersji Pythonowej). Zapis nie wychodzi z edytora. Po zapisie lista `maps` w menu jest odświeżana, żeby nowa / zmieniona nazwa była od razu widoczna.

### Sterowanie edycją
* `b` stawia budynek albo, jeśli na polu już znajduje się budynek, cyklicznie zmienia jego rodzaj.
* Cyfry zmieniają liczbę jednostek (w zakresie 0–999) w budynku (wpisywanie zatwierdzane natychmiast po wpisaniu trzeciej cyfry albo sekundę po wpisaniu pierwszej lub drugiej cyfry; wpisywane cyfry wyświetlane od razu w polu liczby jednostek; bez budynku wpisywanie nic nie zmienia; ciąg cyfr związany z polem, na którym rozpoczęto wpisywanie — wskazanie innego pola w trakcie akceptuje wpisaną wartość).
* `o` cyklicznie zmienia właściciela budynku (bez działania, jeśli na polu nie ma budynku).
* `t` wstawia utrudnienie lub cyklicznie zmienia jego rodzaj.
* `m` wstawia most lub cyklicznie go obraca (jeśli na polu sąsiadującym jest most skierowany w stronę bieżącego pola, wstawiany most dostaje ten sam kierunek; w przeciwnym razie obrót tak, by łączył dwóch przeciwległych sąsiadów o tej samej wysokości, możliwie najwyższych).
* `r` wstawia podjazd (o kierunku między dwoma przeciwległymi sąsiadami o różnych wysokościach, jeśli istnieją) lub cyklicznie go obraca.
* `[` zmniejsza wysokość terenu o 1 (nic przy wysokości 0); `]` zwiększa wysokość terenu o 1 (nic przy wysokości 15).
* `Del` lub prawy przycisk myszy kasuje obiekt.
* Wstawienie obiektu nadpisuje obiekt, który znajdował się na polu wcześniej.
* Edytor wyświetla legendę z opisem działania klawiszy.
* Działka i wieże lecznicze wyświetlane są wraz z zasięgiem (wieże lecznicze zgodnie z regułami gry, zależnie od liczby jednostek; neutralne wieże lecznicze bez zasięgu).
* Pierwszy wstawiony budynek jest początkowo neutralną bazą czołgową z zerem jednostek; zmiana własności budynków (typy, kolory, liczby jednostek) jest zapamiętywana i nadawana nowo wstawianym budynkom. Analogicznie zapamiętywany jest rodzaj ostatnio wybranego utrudnienia.

### Zapis, odczyt i walidacja
* `l` ładuje mapę z pliku (nakładka z nazwami z katalogu `maps` do wskazania, w tym samym oknie); `s` zapisuje mapę do pliku (wpisanie nazwy albo wybór istniejącej; wcześniej nadana nazwa wyświetla się jako pierwsza i jest wyróżniona); `ctrl`+`s` zapisuje pod wcześniej wybraną nazwą (albo działa jak `s`, gdy nazwy jeszcze nie wybrano); `ctrl`+`n` czyści / tworzy nową mapę (bez żądania potwierdzenia).
* Nakładki list (`l`/`s`) oraz wpisywanie nazwy i cyfr realizowane są zdarzeniami klawiatury macroquad w tym samym oknie — nie ma osobnego okna dialogowego jak w implementacji Pythonowej (szczegóły obsługi `Del`, modyfikatora `ctrl` i układu klawiatury do rozstrzygnięcia przy implementacji).
* Jeśli mapa jest niezgodna z regułami gry, na ekranie wypisywane są błędy (czerwonym kolorem) — ten sam zestaw co w [specification_of_map_editor.md](specification_of_map_editor.md): budynek na wodzie, most nad za wysokim lądem, most łączący dwa pola o różnej wysokości, podjazd na polu o innej wysokości niż niższe z łączonych pól, podjazd łączący pola o tych samych wysokościach, brak bazy gracza, brak bazy przynajmniej jednego przeciwnika. Mapę z błędami wciąż można zapisać (i potem wczytać).

### Widok i rozmiar planszy
Widok i jego sterowanie są takie jak w grze, poza kolidującymi cechami: bez `WASD` (kolizja klawisza `s`) i bez panoramowania prawym przyciskiem (bo kasuje obiekt). Stałe edytora mieszkają w `constants.rs` (każda z komentarzem wskazującym sekcję [rules.md](rules.md)); nowe liczby nie są hardkodowane w logice ani w rendererze (zasada wspólna z [specification.md](specification.md), sekcja „Kod”).

Nowo utworzona plansza ma wymiary 256 na 256 i w większości składa się z wody; na środku prostokąt 20 na 13 o wysokości 1, widok wycentrowany. Przy zapisie puste (sama woda bez obiektów) początkowe oraz końcowe wiersze i kolumny są usuwane (minimalny rozmiar zapisanej mapy to 1 na 1). Przy odczycie plansze poniżej 256 na 256 są poszerzane (dopełniane do 256 na 256) o wiersze i kolumny po równo na początku/końcu; widok centrowany. Ze względu na geometrię siatki heksów (pionowe przesunięcie kolumny zależy od parzystości jej numeru) przesunięcie kolumnowe przy usuwaniu i dopełnianiu jest zawsze parzyste: zapis może zachować jedną dodatkową pustą kolumnę po lewej stronie, a dopełnienie dzieli się „po równo” z dokładnością do jednej kolumny (nadwyżka trafia na koniec); na wiersze to ograniczenie nie wpływa.

## Determinizm i czas
Prędkość klatki jest decyzją tej implementacji: `FPS` i `SIM_DT` to stałe w `constants.rs`, a symulacja działa o stałym kroku `SIM_DT` niezależnie od zegara systemowego — czas rzeczywisty służy wyłącznie do zliczania kroków. Symulacja i AI są deterministyczne dla danego seeda (sekcja 13 [rules.md](rules.md)); seed wyznaczany jest z nazwy pliku poziomu, jak w implementacji Pythonowej. Determinizm między implementacjami nie jest wymagany — wymagany jest determinizm wewnątrz tej implementacji. Losowość (szum AI) realizuje `rng.rs` własnym, deterministycznym generatorem, bez dodatkowych crate'ów.

## Katalog maps
Ścieżkę katalogu `maps` wyznaczamy względem katalogu głównego repozytorium, a nie względem katalogu roboczego — to zasada z [specification.md](specification.md) (sekcja „Struktura repozytorium”). Punktem wyjścia jest katalog nadrzędny wobec `env!("CARGO_MANIFEST_DIR")`; przy uruchomieniu z gotowej binarki sprawdzamy kolejne katalogi nadrzędne wobec katalogu binarki. Menu poziomów listuje wszystkie pliki `*.map` z tego katalogu (nazwa pliku jest nazwą poziomu), bez hardkodowania listy.

