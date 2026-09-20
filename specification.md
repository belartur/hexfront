# Hexfront — specyfikacja implementacji gry

## Wstęp
Hexfront to gra komputerowa napisana w Pythonie, przy użyciu bibliotek PyGame. Niniejsza specyfikacja opisuje tę implementację; jej kod i testy leżą w katalogu `python/`.

Reguły gry znajdują się w pliku [rules.md](rules.md).

## Struktura repozytorium
Pliki niezależne od języka implementacji leżą w katalogu głównym repozytorium: reguły ([rules.md](rules.md)), ta specyfikacja, [specification_of_map_editor.md](specification_of_map_editor.md), [specification_of_map_format.md](specification_of_map_format.md) oraz katalog `maps` z planszami (format opisany w tym ostatnim pliku). Implementacja w Pythonie ma własny katalog `python/`: punkty wejścia `main.py`, `editor.py`, `make_maps.py`, pakiet `hexfront/` (logika i UI) oraz testy headless w `tests/`. Kolejne implementacje (np. w innych językach) dostają własne katalogi obok `python/` i nie zmieniają dokumentów ani katalogu `maps`.

Ścieżka katalogu `maps` wyznaczana jest w kodzie względem katalogu głównego repozytorium (stałe `REPO_ROOT` i `MAPS_DIR` w `python/hexfront/constants.py`), dzięki czemu grę, edytor i testy można uruchamiać z dowolnego katalogu roboczego.

## Kod
Kod jest przejrzysty i dobrze udokumentowany, w języku angielskim.
Wszelkie funkcje, metody, klasy, pola, itp. mają dokumentację.
Logika gry jest sensownie oddzielona i niezależna od interfejsu użytkownika (tę regułę można nagiąć w uzasadnionych przypadkach).
Wszelkie stałe są zdefiniowane (najlepiej w osobnym pliku/plikach) i udokumentowane, także można je łatwo zmienić i eksperymentować z innymi wartościami. Stałe dotyczące odległości, zasięgów i prędkości wyrażone są w jednostkach odległości (j) z rules.md; przelicznik j → piksele zdefiniowany jest w jednym miejscu.

## Grafika i interfejs użytkownika
Grafika jest izometryczna. Plansza rysuje się z kodu. Okno gry można skalować.
Obiekty także rysuje się z kodu (w przyszłości możliwe podmienienie na grafikę rastrową).
Obiekty poszczególnych graczy różnią się kolorem (kod rysujący przyjmuje kolor jako argument).

Przy budynkach i pojazdach, po prawej stronie z dołu wyświetla się kółko z białą liczbą po środku oznaczającą ilość jednostek w budynku lub pojeździe. Jeśli w budynku jest przynajmniej maksymalna ilość jednostek, pod liczbą wyświetla się napis 'MAX', który nadal mieści się w kółku. Jeśli w bazie rodzą się jednostki, jest renderowany biały okrąg dopełniający się przez 10 sekund, który nie nachodzi nigdy na numer w tym kółku. Jeśli okrąg się dopełni do końca, w bazie rodzi się 5 jednostek (zgodnie z cyklem produkcji z rules.md). Ląd ma kolor szary, a woda jasny niebieski.

Pociski są rysowane z ciemnym konturem o szerokości 1 px ekranu; promień zwykłego pocisku wynosi 3 px, a rakiety 5 px, niezależnie od zoomu.

Po wysłaniu pojazdu jego droga rysowana jest przerywaną kreską, która znika za pojazdem.
Gdy pojazd lub budynek traci x jednostek, to wyświetla się biała liczba -x lecąca przez 2 sekundy do góry od liczby oznaczającej ilość jednostek w tym pojeździe lub budynku. Wyjątek stanowi wysyłanie pojazdu z budynku, wtedy taka liczba się nie wyświetla. Gdy pojazd zyskuje x jednostek z powodu leczenia, wyświetla się jasnozielone +x. Gdy pojazd strzela w ścianę, wyświetla się biały numerek oznaczający, który to z kolei strzał tej jednostki w tę ścianę (numeracja rozpoczyna się od nowa, gdy jednostka zacznie ostrzeliwać inną ścianę).

Zasięgi działek są renderowane jako białe, a wież leczących jako jasnozielone. Zasięgi mają spory procent przezroczystości. Każdy zasięg otoczony jest kreską w kolorze swojego wypełnienia, wyraźnie mniej przezroczystą niż wypełnienie. Wypełnienia wszystkich zasięgów rysowane są w pierwszym przejściu, a kreski w drugim — dzięki temu kreska przykrywa wypełnienia innych zasięgów i pozostaje czytelna tam, gdzie zasięgi się nakładają. Zasięgi leczenia przez bufory także renderują się jako jasnozielone i przesuwają się one wraz z ruchem tego pojazdu. Zasięgi wykrywania wrogich pojazdów nie są zaznaczane.

Nieprzezroczysta geometria planszy korzysta ze wspólnego bufora głębokości: powierzchnie pól, skarpy, rampy, mosty, budynki, utrudnienia i pojazdy zasłaniają się na poziomie pikseli, nie według środka całego obiektu. Dla punktu świata przyjmujemy współrzędną głębokości D = (x + y)·ISO_SIN + z, gdzie z jest rzeczywistą wysokością rysowanego punktu w pikselach świata. Na jednym promieniu widzenia większe D oznacza punkt bliższy obserwatorowi. D jest interpolowane liniowo na rzutowanych płaskich powierzchniach; widoczny pozostaje najbliższy fragment. Wnętrza ogólnych wielokątów próbkujemy w środkach pikseli. Poziome wierzchy pól grupujemy według wysokości: PyGame rasteruje wspólną maskę wypełnień, a następnie wspólną maskę siatki. Dla obu masek głębokość pochodzi z równania tej samej płaszczyzny w środku piksela, również na brzegach zaokrąglonego obrysu. Siatka ma szerokość 2 px ekranu przy każdym zoomie i nie jest pomijana przy oddaleniu. Obrysy wielokątów używają równania ich powierzchni na całej szerokości. Linie poziomych znaków używają płaszczyzny, na której leżą; pozostałe linie mają głębokość interpolowaną wzdłuż odcinka, także próbkowaną w środku piksela. Szerokość linii pozostaje ekranowa. Dzięki temu własna powierzchnia nie wycina znaków, ale bliższy teren nadal je zasłania. Remisy współpłaszczyznowych fragmentów rozstrzyga stała kolejność: podłoże przed detalami i obrysami, przy jedynie numerycznej tolerancji porównań. Nie stosujemy sztucznego podnoszenia kluczy obiektów ani końcowego przebiegu ramp ponad całą sceną.

Pojazd nad własnym płaskim podłożem pozostaje widoczny także przy dalszej krawędzi pola; bliższa skarpa może zasłaniać pojazd znajdujący się za nią. Wysokość pojazdu uwzględnia grunt, nachylenie rampy lub pokład mostu zgodnie z trasą przejazdu. Pokład zasłania przejeżdżające pod nim pojazdy, ale nie pojazdy na nim. Rampa może być częściowo zasłonięta przez bliższe powierzchnie, a sama zasłania powierzchnie leżące za nią.

Cienie pojazdów są półprzezroczystymi przyciemnieniami podłoża (czarny kolor, alfa 70/255, promień 14 j, obrys z 14 wierzchołków). Są oddzielone od nakładki zasięgów i nakładane przed pojazdami, poza cache terenu. Przyciemniają wyłącznie widoczne piksele o głębokości powierzchni przyjmującej cień, bez zapisywania głębokości; nie przyciemniają korpusów, budynków ani bliższych skarp. Na rampie podążają za nachyleniem pasa. Pojazd jadący po moście rzuca cień na pokład, a jadący pod nim — na grunt lub wodę; helikopter nad fragmentem mostu rzuca cień na pokład niezależnie od trasy. Wizualna powierzchnia pokładu leży 5 j powyżej nominalnej wysokości mostu; cień używa tej samej powierzchni. Fragmenty cienia poza powierzchnią przyjmującą są przycinane testem głębokości, nie przenoszone na niższe powierzchnie.


Gra i edytor współdzielą renderer. Obliczenia pikselowe wykorzystują NumPy (zależność uruchomieniowa obok PyGame). Zachowany jest culling widoku; obliczenia prymitywów ograniczamy do ich prostokątów ekranowych. Obraz i głębokość nieruchomego terenu są buforowane, a zmiana kamery, rozmiaru okna, planszy, wysokości lub ramp unieważnia cache. Ruchome obiekty i nakładki nie trafiają do cache. Przy pełnym przerysowaniu wierzchy pól i ich siatka przechodzą zbiorczy test głębokości dla każdej wysokości, bez osobnych tablic NumPy na każdą krawędź. Pomijamy geometrię nieobecnych skarp i wierzchów poza ekranem. Zmiany widoku, także podpikselowe, są renderowane bez zaokrąglania kamery lub skalowania starej klatki. Pełne przerysowanie po zmianie widoku jest droższe od klatki z nieruchomą kamerą; ta implementacja programowa nie gwarantuje docelowego FPS podczas panoramowania dużych widoków. Zasięgi i oznaczenia interfejsu zachowują osobne przejścia nakładkowe; cienie uczestniczą w teście głębokości opisanym powyżej, a liczniki jednostek rysowane są na końcu. Skarpy całkowicie poza ekranem odrzucamy przed projekcją wierzchołków, z uwzględnieniem pełnej wysokości ściany. Zapis widocznych kolorów i głębokości odbywa się bezpośrednio pod maską NumPy, bez tworzenia tablic wybranych wartości.

Podjazd nie ma strzałek i nie zajmuje całego hexu — rysowany jest jako węższy pas w kolorze ziemi, biegnący przez środek pola wzdłuż osi podjazdu od krawędzi pola a do krawędzi pola b (na bokach pola p pozostaje zwykły teren). Górna powierzchnia pasa jest rzutem prostokąta w świecie (równoległobokiem na ekranie): jej krótsze krawędzie leżą na środkach krawędzi hexu od strony pól a i b, na wysokościach odpowiednich sąsiadów (krawędź wyznaczona środkiem sąsiada leżącym na jej osi), długie krawędzie są równoległe do osi a→b, więc nachylenie pasa jest proporcjonalne do różnicy wysokości łączonych pól (przy równej wysokości podjazd jest płaski). Korpus podjazdu jest pełny — przestrzeń pod pochyloną powierzchnią do poziomu podstawy wypełnia ciemniejsza ziemia, nie widać pod nim pustki. Liczba jednostek (kółko z liczbą) nad budynkami i pojazdami rysowana jest w osobnym, ostatnim przejściu — ponad wszystkim innym, nigdy zasłonięta przez teren ani obiekty.

Po uruchomieniu gry wyświetla się menu, z poziomami: każdy poziom ma swoją wyświetlaną nazwę (równą nazwie pliku z planszą w katalogu maps).
Po wybraniu poziomu ładuje się on i gra się zaczyna.


## Sterowanie

**Widok:** planszę można przesuwać, przeciągając ją myszą z wciśniętym LMB, klawiszami strzałek, klawiszami WASD oraz przez przytrzymanie kursora na krawędzi ekranu. Przesuwanie jest ograniczone do granic planszy (przy najdalszym przesunięciu, skrajne pole planszy może znaleźć się na środku ekranu). Zoom wykonuje się kółkiem myszy albo klawiszami + i −, w zakresie od 0,5× do 2×.

**Zaznaczanie budynku:** kliknięcie PPM zawsze zaznacza wskazany własny budynek (z dodatnią liczbą jednostek w środku) jako budynek źródłowy; kolejne kliknięcia PPM zmieniają zaznaczenie na inny budynek. Kliknięcie PPM poza własnym budynkiem z jednostkami anuluje zaznaczenie.

**Wysyłanie pojazdu:** kliknięcie LPM zaznacza wskazany własny budynek (z dodatnią liczbą jednostek w środku), o ile żaden inny budynek nie jest zaznaczony. Gdy jakiś budynek jest zaznaczony, kliknięcie LPM na dowolny inny budynek wysyła pojazd z jednostkami z budynku zaznaczonego do wskazanego — pozwala to również na przesyłanie jednostek między własnymi budynkami. Jeśli nie istnieje droga, pojazd nie jest wysyłany, a zaznaczenie zostaje. Podgląd trasy do budynku wskazanego kursorem rysowany jest na bieżąco. Zaznaczenie można anulować klawiszem Esc albo kliknięciem PPM poza własnym budynkiem z jednostkami. Kursor myszy wskazuje najbliższy budynek, którego środek znajduje się w odległości co najwyżej 150 j; gdy wszystkie budynki są dalej, nie wskazuje nic (puste pola nigdy się nie zaznaczają, a remis rozstrzygany jest deterministycznie po współrzędnych pola). Odległość mierzona jest w świecie gry na wysokości danego budynku (niezależnie od zoomu). Lecz gdy jest przyciśnięty klawisz `alt`, to wskazuje to pole na które wskazywałby gdyby wszystkie pola znajdowałyby się na wysokości zero. Odnosi się to zarówno do poziomów jak i do edytora plansz.

**Menu poziomów:** poziomy wyświetlane są w zwartej, trzykolumnowej siatce — po jednej klikalnej komórce na planszę — a poziom wybiera się kliknięciem na jego wyświetlaną nazwę. Zbyt długie nazwy są przycinane wielokropkiem do szerokości kolumny. Gdy wiersze nie mieszczą się na ekranie, siatkę przewija się kółkiem myszy albo klawiszami Góra/Dół (także PageUp/PageDown); przy przewijanej liście po prawej stronie widoczny jest pasek przewijania. Podczas gry klawisz Esc powraca do menu, chyba że aktualnie jest zaznaczony budynek (wtedy Esc anuluje zaznaczenie; patrz wysyłanie pojazdu w tej sekcji).

**Pauza:** klawisz P wstrzymuje i wznawia grę.

## Dźwięk
Brak dźwięku (w przyszłości to się może zmienić).

## Parametry

Wszystkie wartości liczbowe gry (odległości, promienie, zasięgi, itd.) są zdefiniowane w [rules.md](rules.md) w jednostkach odległości (j). Ta sekcja określa wyłącznie odwzorowanie jednostek na ekran:

* przelicznik: **1 j = 1 px** przy skali widoku 1:1 — jedna stała w kodzie; zoom i skalowanie okna dotyczą tylko renderingu,
* bok sześciokąta: **36 j** (układ flat-top) — jedyna wartość geometryczna spoza zasad, potrzebna do przeliczenia współrzędnych heksów na pozycje w świecie gry; pozostałe wymiary pola wynikają z niej (√3).

FPS = 1/60

## Plansze i edytor plansz
Edytor plansz jest osobną aplikacją o specyfikacji opisanej w [specification_of_map_editor.md](specification_of_map_editor.md).
Plansze zapisywane są w katalogu maps w plikach o rozszerzeniu `map`, każda w osobnym pliku.

## Format pliku planszy
Format pliku planszy jest wspólny dla wszystkich implementacji i opisany jest w [specification_of_map_format.md](specification_of_map_format.md).

W implementacji Python format ten obsługuje `python/hexfront/mapfile.py`: tabele `BUILDING_CODES` i `OBSTACLE_CODES` oraz stałe `BRIDGE_CODE_BASE`, `RAMP_CODE_BASE`, `OWNER_CODE_*` i `MAX_SAVED_UNITS`. Rozszerzenie pliku i katalog plansz wyznaczają stałe `MAP_EXTENSION` i `MAPS_DIR` z `python/hexfront/constants.py`, a zgodność kodu z formatem pilnuje test `python/tests/test_mapfile.py` (uruchamiany jako `cd python && python3 -m tests.test_mapfile`).
