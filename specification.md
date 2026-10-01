# Hexfront — specyfikacja implementacji gry

## Wstęp
Hexfront to gra komputerowa o zasadach opisanych w [rules.md](rules.md). Ten dokument opisuje część wspólną wszystkich implementacji: wymagania wspólne dla każdej z nich (wygląd, sterowanie, parametry, obsługa plansz).

Specyfikacja implementacji:
* [specification_rust.md](specification_rust.md) — implementacja w Rust (stabilny toolchain rustc/cargo i biblioteka macroquad), kod w katalogu `rust/`.

Dokumenty wspólne dla wszystkich implementacji:
* [rules.md](rules.md) — zasady gry (jedyne źródło liczb i reguł gry),
* [specification_of_map_format.md](specification_of_map_format.md) — binarny format pliku planszy,
* ten dokument — część wspólna specyfikacji implementacji.

Liczb i reguł nie kopiuje się między dokumentami: wartości liczbowe gry opisuje wyłącznie [rules.md](rules.md) w jednostkach odległości (j), format pliku planszy wyłącznie [specification_of_map_format.md](specification_of_map_format.md), a szczegóły danego języka wyłącznie specyfikacja tego języka.

Ten dokument opisuje **wymagania wspólne** dla każdej implementacji, a nie sposób ich spełnienia. Zapisuje decyzje i kontrakt; szczegóły wykonania (wartości w px, bufory, algorytmy) należą do dokumentacji kodu konkretnej implementacji.

## Struktura repozytorium
Pliki niezależne od języka implementacji leżą w katalogu głównym repozytorium: reguły ([rules.md](rules.md)), specyfikacje ([specification.md](specification.md), [specification_rust.md](specification_rust.md), [specification_of_map_format.md](specification_of_map_format.md)) oraz katalog `maps` z planszami (format opisany w tym ostatnim pliku). Kod implementacji języka ma osobny katalog — obecnie `rust/`.

Ścieżka katalogu `maps` wyznaczana jest w kodzie względem katalogu głównego repozytorium, a nie względem katalogu roboczego, dzięki czemu grę, edytor i testy można uruchamiać z dowolnego katalogu. Kolejne implementacje dostałyby własne katalogi obok `rust/` i nie zmieniałyby dokumentów ani katalogu `maps`.

## Kod
Kod jest przejrzysty i dobrze udokumentowany, w języku angielskim.
Wszelkie funkcje, metody, klasy, pola, itp. mają dokumentację.
Logika gry jest sensownie oddzielona i niezależna od interfejsu użytkownika (tę regułę można nagiąć w uzasadnionych przypadkach).
Wszelkie stałe są zdefiniowane (najlepiej w osobnym pliku/plikach) i udokumentowane, także można je łatwo zmienić i eksperymentować z innymi wartościami. Stałe dotyczące odległości, zasięgów i prędkości wyrażone są w jednostkach odległości (j) z rules.md; przelicznik j → piksele zdefiniowany jest w jednym miejscu (nazwę stałej podaje specyfikacja języka).
Kod jest pisany z dbałością o wydajność; ten dokument nie stawia jednak twardych wymagań czasowych — techniki wydajnościowe (culling, cache itp.) są decyzjami implementacji, opisanymi w jej specyfikacji.
Decyzje zapisane w specyfikacji danej implementacji (nazwy modułów, nazwy stałych, rozwiązania techniczne) można zmieniać w trakcie implementacji, jeśli ma się ku temu konkretny powód — zawsze wraz z aktualizacją tej specyfikacji w tym samym commicie.

## Grafika i interfejs użytkownika
Grafika jest izometryczna. Plansza rysuje się z kodu, bez assetów rastrowych. Okno gry można skalować.
Obiekty także rysuje się z kodu (w przyszłości możliwe podmienienie na grafikę rastrową).
Obiekty poszczególnych graczy różnią się kolorem (kod rysujący przyjmuje kolor jako argument).
Ląd ma kolor szary, a woda jasny niebieski.

**Liczniki jednostek.** Przy budynkach i pojazdach, po prawej stronie z dołu wyświetla się kółko z białą liczbą pośrodku oznaczającą ilość jednostek. Gdy w budynku jest przynajmniej maksymalna ilość jednostek, pod liczbą wyświetla się napis 'MAX', który nadal mieści się w kółku. Liczniki i liczby jednostek rysowane są zawsze na wierzchu — nigdy nie zasłania ich teren ani obiekty.

**Produkcja w bazie.** Jeśli w bazie rodzą się jednostki, obok licznika renderowany jest biały okrąg dopełniający się przez 10 sekund, który nie nachodzi nigdy na numer w kółku. Jeśli okrąg się dopełni do końca, w bazie rodzi się 5 jednostek (zgodnie z cyklem produkcji z [rules.md](rules.md)).

**Pociski** są rysowane z ciemnym konturem; zwykły pocisk i rakieta różnią się wielkością. Pod budynkiem, pojazdem i pociskiem wypisywane są liczby z liczbą ostatnio utraconych (`-x`) lub odzyskanych (`+x`) jednostek.

**Zasięgi.** Zasięgi działek są renderowane jako białe, a wież leczących jako jasnozielone, z dość dużą przezroczystością. Każdy zasięg otoczony jest kreską **w kolorze gracza, do którego należy budynek** (białą dla budynku neutralnego), wyraźnie mniej przezroczystą niż wypełnienie. Nakładanie się dwóch zasięgów tego samego rodzaju nie kumuluje krycia — wyglądają jak jeden zasięg, a nie jak dwa. Zasięgi nie są przycinane testem głębokości: nic nie może ich zasłonić ani przykryć, więc pozostają w pełni widoczne także pod obiektami, za skarpą czy pod pokładem mostu. Obrysy rysowane są po wypełnieniach, bez testu głębokości — kreska przykrywa wypełnienia innych zasięgów i pozostaje czytelna tam, gdzie zasięgi się nakładają. Zasięgi leczenia przez bufory renderują się tak samo jasnozielone i przesuwają wraz z ruchem tego pojazdu. Zasięgi wykrywania wrogich pojazdów nie są zaznaczane.

**Zasłanianie.** Nieprzezroczysta geometria planszy korzysta ze wspólnego bufora głębokości: powierzchnie pól, skarpy, rampy, mosty, budynki, utrudnienia i pojazdy zasłaniają się na poziomie pikseli, nie według środka całego obiektu. Dla punktu świata przyjmujemy współrzędną głębokości **D = (x + y)·k + z**, gdzie k jest stałym współczynnikiem rzutu izometrycznego (sinus kąta nachylenia), a z jest rzeczywistą wysokością rysowanego punktu w pikselach świata. Na jednym promieniu widzenia większe D oznacza punkt bliższy obserwatorowi. Remisy współpłaszczyznowych fragmentów rozstrzyga stała kolejność: podłoże przed detalami i obrysami. Nie stosujemy sztucznego podnoszenia kluczy obiektów ani końcowego przebiegu ramp ponad całą scenę.

**Cienie.** Cień to półprzezroczyste przyciemnienie podłoża, rysowane oddzielnie od nakładki zasięgów i nakładane przed pojazdami. Przyciemnia wyłącznie widoczne piksele o głębokości powierzchni przyjmującej cień; nie przyciemnia korpusów, budynków ani bliższych skarp. Na rampie podąża za nachyleniem pasa. Pojazd jadący po moście rzuca cień na pokład, a jadący pod nim — na grunt lub wodę; helikopter nad fragmentem mostu rzuca cień na pokład niezależnie od trasy. Fragmenty cienia poza powierzchnią przyjmującą są przycinane testem głębokości, nie przenoszone na niższe powierzchnie.

**Podjazd** nie ma strzałek i nie zajmuje całego hexu — rysowany jest jako węższy pas w kolorze ziemi, biegnący przez środek pola wzdłuż osi podjazdu od krawędzi pola a do krawędzi pola b. Korpus podjazdu jest pełny — przestrzeń pod pochyloną powierzchnią wypełnia ciemniejsza ziemia, nie widać pod nim pustki.

**Wybuch zniszczonego pojazdu** to efekt czysto prezentacyjny: gdy liczba jednostek spadnie do zera, w miejscu zniszczenia pojawia się seria cząstek. Barwy ognia i iskier są lekko domieszane kolorem gracza, żeby było widać, czyja to była jednostka. Dwa zniszczenia tego samego pojazdu wyglądają różnie, ale ta sama plansza odtworzona zawsze daje tę samą sekwencję wybuchów. Efekt nigdy nie wpływa na symulację.

## Sterowanie

**Widok:** planszę można przesuwać, przeciągając ją myszą z wciśniętym LMB, klawiszami strzałek, klawiszami WASD oraz przez przytrzymanie kursora na krawędzi ekranu. Przesuwanie jest ograniczone do granic planszy (przy najdalszym przesunięciu, skrajne pole planszy może znaleźć się na środku ekranu). Zoom wykonuje się kółkiem myszy albo klawiszami + i −.

**Zaznaczanie budynku:** kliknięcie PPM zawsze zaznacza wskazany własny budynek (z dodatnią liczbą jednostek w środku) jako budynek źródłowy; kolejne kliknięcia PPM zmieniają zaznaczenie na inny budynek. Kliknięcie PPM poza własnym budynkiem z jednostkami anuluje zaznaczenie.

**Wysyłanie pojazdu:** kliknięcie LPM zaznacza wskazany własny budynek (z dodatnią liczbą jednostek w środku), o ile żaden inny budynek nie jest zaznaczony. Gdy jakiś budynek jest zaznaczony, kliknięcie LPM na dowolny inny budynek wysyła pojazd z jednostkami z budynku zaznaczonego do wskazanego — pozwala to również na przesyłanie jednostek między własnymi budynkami. Jeśli nie istnieje droga, pojazd nie jest wysyłany, a zaznaczenie zostaje. Podgląd trasy do budynku wskazanego kursorem rysowany jest na bieżąco. Zaznaczenie można anulować klawiszem Esc albo kliknięciem PPM poza własnym budynkiem z jednostkami.

**Wskazywanie budynku kursorem:** kursor wskazuje najbliższy budynek, którego środek znajduje się w dostatecznie małej odległości (promień przyciągania jest decyzją implementacji); gdy wszystkie budynki są dalej, nie wskazuje nic. Puste pola nigdy się nie zaznaczają, a remis rozstrzygany jest deterministycznie po współrzędnych pola. Odległość mierzona jest w świecie gry na wysokości danego budynku (niezależnie od zoomu). Lecz gdy jest przyciśnięty klawisz `alt`, to wskazuje to pole, na które wskazywałby, gdyby wszystkie pola znajdowałyby się na wysokości zero. Odnosi się to zarówno do poziomów, jak i do edytora plansz.

**Menu poziomów:** poziomy wyświetlane są w zwartej siatce — po jednej klikalnej komórce na planszę — a poziom wybiera się kliknięciem na jego wyświetlaną nazwę (zbyt długie nazwy są przycinane wielokropkiem do szerokości kolumny). Gdy poziomy nie mieszczą się na ekranie, siatkę przewija się kółkiem myszy albo klawiszami Góra/Dół. Podczas gry klawisz Esc powraca do menu, chyba że aktualnie jest zaznaczony budynek (wtedy Esc anuluje zaznaczenie; patrz wysyłanie pojazdu w tej sekcji).

**Pauza:** klawisz P wstrzymuje i wznawia grę.

## Dźwięk
Gra odtwarza dźwięki zdarzeń bojowych: wybuch zniszczonego pojazdu, strzały działek i pojazdów oraz trafienia pocisków. Dźwięk jest wyłącznie elementem prezentacji — żadna zasada gry nie zależy od tego, co słychać, a symulacja nie bierze udziału w syntezie dźwięku ani w jego odtwarzaniu.

Dźwięk zależy od odległości od miejsca zdarzenia (mierzonej w j, niezależnie od zoomu): blisko słychać pełną głośność, dalej dźwięk cichnie, a poza zasięgiem słyszalności nie jest w ogóle odtwarzany. W jednym kroku symulacji odtwarza się ograniczona liczba dźwięków, a najgłośniejsze są te z najbliższych zdarzeń, więc duża bitwa nie przerodzi się w szum. Ten sam dźwięk nie może zabrzmieć dwa razy w krótkim odstępie czasu (inaczej seria strzałów jednego działka zlewa się w jeden ciągły buczek).

Sposób przygotowania dźwięków, wybór biblioteki i sposób ich przechowywania są decyzją każdej implementacji.

## Parametry

Wszystkie wartości liczbowe gry (odległości, promienie, zasięgi, itd.) są zdefiniowane w [rules.md](rules.md) w jednostkach odległości (j). Ta sekcja określa wyłącznie odwzorowanie jednostek na ekran:

* przelicznik: **1 j = 1 px** przy skali widoku 1:1 — jedna stała w każdej implementacji (nazwę podaje specyfikacja języka); zoom i skalowanie okna dotyczą tylko renderingu,
* bok sześciokąta: **36 j** (układ flat-top) — jedyna wartość geometryczna spoza zasad, potrzebna do przeliczenia współrzędnych heksów na pozycje w świecie gry; pozostałe wymiary pola wynikają z niej (√3).

Prędkość klatki (FPS) nie jest częścią kontraktu — jest decyzją każdej implementacji, opisaną w specyfikacji tego języka.

## Plansze
Plansze zapisywane są w katalogu maps w plikach o rozszerzeniu `map`, każda w osobnym pliku. Każda implementacja listuje katalog `maps` dynamicznie, a nazwa pliku jest wyświetlaną nazwą poziomu w menu — lista poziomów nie jest hardkodowana.

Edytor plansz jest integralną częścią tej samej aplikacji (bez osobnego programu) i opisany jest w [specification_rust.md](specification_rust.md) (sekcja „Edytor plansz”).

## Format pliku planszy
Format pliku planszy jest wspólny dla wszystkich implementacji i opisany jest w [specification_of_map_format.md](specification_of_map_format.md).

## Zgodność implementacji
Implementacja realizuje zasady ([rules.md](rules.md)) i format pliku planszy ([specification_of_map_format.md](specification_of_map_format.md)), a wygląd, sterowanie i parametry opisane w tym dokumencie tworzą jej kontrakt. Poza nim implementacja jest niezależna wewnętrznie: może różnić się podziałem modułów, nazwami, wydajnością i rozwiązaniami technicznymi. Kolejne implementacje powstające w repozytorium są wobec siebie niezależne i nie są dla siebie wzorcem — wiąże je wyłącznie ten wspólny kontrakt, `rules.md` i format planszy.

