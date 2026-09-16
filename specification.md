# Hexfront — specyfikacja implementacji gry

## Wstęp
Hexfront to gra komputerowa napisana w Pythonie, przy użyciu bibliotek PyGame.

Reguły gry znajdują się w pliku [rules.md](rules.md).

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
Plik planszy jest binarny, wszystkie liczby wielobajtowe zapisane są little-endian (młodsza wartość na młodszych bitach — młodszy bajt słowa jako pierwszy w pliku). Znajdują się w nim, kolejno, następujące informacje:
* Wymiary planszy (2 bajty): liczba kolumn k (1 bajt) i wierszy w (1 bajt), każda z zakresu 1–255 (maksimum wynika z 1 bajta na wymiar; `0` jest odrzucane przy odczycie jako pusta plansza).
* k·w liczb 4-bitowych kodujących wysokości kolejnych pól planszy (0–15, 0 to woda) w kolejności row-major: indeks `r·k+q`. Zapisane są na `ceil(k·w/2) = (k·w+1)//2` bajtach, po dwa pola na bajt w porządku little-endian: wcześniejsze pole pary na młodszych 4 bitach (low nibble, bity 3–0), późniejsze na starszych 4 bitach (high nibble, bity 7–4). Gdy k·w jest nieparzyste, starsze 4 bity (high nibble) ostatniego bajta przechowują zero (padding).
* Obiekty znajdujące się na planszy, jeden rekord za drugim aż do końca pliku (bez licznika ani terminatora). Liczba użytych bajtów zależy od typu obiektu. Pierwsze 2 bajty kodują położenie obiektu (1 bajt kolumnę `q` i 1 bajt wiersz `r`). Trzeci bajt koduje typ obiektu, zaś kolejne 2 bajty (wyłącznie w przypadku budynków) jego własności:
  * Liczby z zakresu 0–19 kodują budynek wraz z jego rodzajem (odwzorowanie kod → rodzaj definiuje `BUILDING_CODES` w `hexfront/mapfile.py`; kody 8–19 są obecnie nieużywane). Wtedy kolejne 2 bajty to jedno 16-bitowe słowo little-endian `props = (owner << 10) | units` (pierwszy bajt w pliku to bity 7–0, drugi bity 15–8): numer właściciela budynku na 6 starszych bitach (bity 15–10, wartości: 0 — neutralny, 1 — niebieski, 2 — czerwony, 3 — zielony, 4 — żółty; wartości 5–63 rezerwowe) i początkowa liczba jednostek w budynku na 10 młodszych bitach (bity 9–0, zakres mapy 0–999). 10 bitów mieści fizycznie 0–1023, więc wartości 1000–1023 są przy odczycie sprowadzane z ostrzeżeniem do 999.
  * Liczby z zakresu 20–22 kodują fragment pokładu mostu: `kod − 20` to oś geometryczna mostu `0–2` (kierunek mostu modulo 3, por. `BRIDGE_CODE_BASE` w `hexfront/mapfile.py`). Cały most zapisywany jest jako jeden rekord na każdy fragment pokładu; przy odczycie fragmenty są składane w całe mosty przez `rebuild_bridges()`.
  * Liczby z zakresu 23–25 kodują podjazd: `kod − 23` to oś geometryczna `0–2` pary łączonych przeciwległych sąsiadów (por. `RAMP_CODE_BASE` w `hexfront/mapfile.py`); przy odczycie końce to `a = neighbor(tile, oś)` i `b = neighbor(tile, oś+3)`.
  * Liczby od 26 w górę kodują utrudnienia wraz z rodzajem (odwzorowanie kod → rodzaj definiuje `OBSTACLE_CODES` w `hexfront/mapfile.py`, obecnie 26 — ściana, 27 — mina lądowa, 28 — mina wodna, 29 — pułapka ogniowa, 30 — pułapka lodowa; nieznane kody są przy odczycie pomijane z ostrzeżeniem).