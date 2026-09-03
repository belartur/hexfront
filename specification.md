# War Regions — specyfikacja implementacji gry

## Wstęp
War Regions to gra komputerowa napisana w Pythonie, przy użyciu bibliotek PyGame.

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

Po wysłaniu pojazdu jego droga rysowana jest przerywaną kreską, która znika za pojazdem.
Gdy pojazd lub budynek traci x jednostek, to wyświetla się biała liczba -x lecąca przez 2 sekundy do góry od liczby oznaczającej ilość jednostek w tym pojeździe lub budynku. Wyjątek stanowi wysyłanie pojazdu z budynku, wtedy taka liczba się nie wyświetla. Gdy pojazd zyskuje x jednostek z powodu leczenia, wyświetla się jasnozielone +x. Gdy pojazd strzela w ścianę, wyświetla się biały numerek oznaczający, który to z kolei strzał tej jednostki w tę ścianę (numeracja rozpoczyna się od nowa, gdy jednostka zacznie ostrzeliwać inną ścianę).

Zasięgi działek są renderowane jako białe, a wież leczących jako jasnozielone. Zasięgi mają spory procent przezroczystości. Każdy zasięg otoczony jest kreską w kolorze swojego wypełnienia, wyraźnie mniej przezroczystą niż wypełnienie. Wypełnienia wszystkich zasięgów rysowane są w pierwszym przejściu, a kreski w drugim — dzięki temu kreska przykrywa wypełnienia innych zasięgów i pozostaje czytelna tam, gdzie zasięgi się nakładają. Zasięgi leczenia przez bufory także renderują się jako jasnozielone i przesuwają się one wraz z ruchem tego pojazdu. Zasięgi wykrywania wrogich pojazdów nie są zaznaczane.

Plansza rysowana jest algorytmem malarza, pole po polu, od najdalszych do najbliższych. Głębia pola uwzględnia zarówno pozycję na planszy, jak i wysokość: D = (x + y)·ISO_SIN + wysokość·ELEVATION_PX (wynika to z rzutu izometrycznego, w którym wyższy teren jest bliżej kamery). Każde pole rysowane jest w całości: najpierw ścianki (skarpy), potem wierzch, następnie zawartość — podjazd, utrudnienie i budynek (w tej kolejności). Dzięki wspólnemu kluczowi głębi ścianki są zawsze przyklejone do swojego pola: zasłaniają niższy teren znajdujący się za nimi, a są zasłaniane przez pola przed nimi; wyższy teren z tła pozostaje widoczny nad niższym terenem w tle pierwszym planu. Na końcu rysowane są pomosty mostów, unoszące się ponad całym terenem. Pojazdy rysowane są w osobnym przejściu, po całym terenie. Liczba jednostek (kółko z liczbą) nad budynkami i pojazdami rysowana jest w osobnym, ostatnim przejściu — ponad wszystkim innym, nigdy zasłonięta przez teren ani obiekty.

Po uruchomieniu gry wyświetla się menu, z poziomami: każdy poziom ma swoją wyświetlaną nazwę.
Po wybraniu poziomu ładuje się on, pokazując mapę.


## Sterowanie

**Widok:** planszę można przesuwać, przeciągając ją myszą z wciśniętym LMB, klawiszami strzałek, klawiszami WASD oraz przez przytrzymanie kursora na krawędzi ekranu. Zoom wykonuje się kółkiem myszy albo klawiszami + i −, w zakresie od 0,5× do 2×.

**Zaznaczanie budynku:** kliknięcie PPM zawsze zaznacza wskazany własny budynek (z dodatnią liczbą jednostek w środku) jako budynek źródłowy; kolejne kliknięcia PPM zmieniają zaznaczenie na inny budynek. Kliknięcie PPM poza własnym budynkiem z jednostkami anuluje zaznaczenie.

**Wysyłanie pojazdu:** kliknięcie LPM zaznacza wskazany własny budynek (z dodatnią liczbą jednostek w środku), o ile żaden inny budynek nie jest zaznaczony. Gdy jakiś budynek jest zaznaczony, kliknięcie LPM na dowolny inny budynek wysyła pojazd z jednostkami z budynku zaznaczonego do wskazanego — pozwala to również na przesyłanie jednostek między własnymi budynkami. Jeśli nie istnieje droga, pojazd nie jest wysyłany, a zaznaczenie zostaje. Podgląd trasy do budynku wskazanego kursorem rysowany jest na bieżąco. Zaznaczenie można anulować klawiszem Esc albo kliknięciem PPM poza własnym budynkiem z jednostkami.

**Menu poziomów:** poziom wybiera się kliknięciem na jego wyświetlaną nazwę. Podczas gry klawisz Esc powraca do menu, chyba że aktualnie jest zaznaczony budynek (wtedy Esc anuluje zaznaczenie; patrz wysyłanie pojazdu w tej sekcji).

**Pauza:** klawisz P wstrzymuje i wznawia grę.

## Dźwięk
Brak dźwięku (w przyszłości to się może zmienić).

## Parametry

Wszystkie wartości liczbowe gry (odległości, promienie, zasięgi, itd.) są zdefiniowane w [rules.md](rules.md) w jednostkach odległości (j). Ta sekcja określa wyłącznie odwzorowanie jednostek na ekran:

* przelicznik: **1 j = 1 px** przy skali widoku 1:1 — jedna stała w kodzie; zoom i skalowanie okna dotyczą tylko renderingu,
* bok sześciokąta: **36 j** (układ flat-top) — jedyna wartość geometryczna spoza zasad, potrzebna do przeliczenia współrzędnych heksów na pozycje w świecie gry; pozostałe wymiary pola wynikają z niej (√3).

FPS = 1/60

### Generowanie planszy
Jest jedna przykładowa plansza generowana z kodu.
W przyszłości plansze będą projektowane w PyTMX (na razie jednak nie jest to obsługiwane i PyTMX nie jest wymagany).
