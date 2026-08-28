# War Regions — Zasady Gry

## 1. Mapa

Gra War Regions rozgrywa się na planszy zbudowanej z sąsiadujących (czyli mających wspólny bok) **sześciokątnych pól**.

W rozgrywce bierze udział od 2 do 4 graczy, w tym 1 sterowany przez człowieka, i reszta przez komputer.

Do każdego pola na mapie przypisana jest niezmienna w trakcie gry wysokość (nieujemna liczba całkowita do 15).
Wysokość 0 to woda, a większa to ląd.

Każde pole może być puste, albo może się na nim znajdować jeden z niżej wymienionych obiektów.

Na każdym polu z lądem może się znajdować jedno z:

* budynek,
* podjazd,
* fragment mostu,
* ściana,
* mina lądowa,
* pułapka (ogniowa lub lodowa).

Na polu z wodą (o wysokości 0) może się znajdować jedno z:

* podjazd,
* fragment mostu,
* ściana,
* mina wodna.

Każdy z budynków przynależy do dokładnie jednego z graczy albo jest neutralny.

Gracz sterowany przez człowieka i przynajmniej 1 z tych sterowanych przez AI na początek mają przynajmniej jedną bazę.

Plansza gry może być dość duża i nie mieścić się na ekranie. Wtedy gracz może przesuwać widok, aby obserwować różne części pola bitwy.


## 2. Cel Gry

Celem gry jest przejęcie kontroli nad wszystkimi budynkami i zniszczenie wszystkich pojazdów na mapie.
Gracz kontrolowany przez człowieka zwycięża po przejęciu wszystkich budynków i zniszczeniu wszystkich wrogich pojazdów.
Jeśli zaś straci on wszystkie budynki, to gra się kończy przegraną.
Gdy AI straci wszystkie jednostki (w budynkach i na pojazdach), to jest ono eliminowane.

Po przejęciu budynku, zaczyna on działać na rzecz nowego właściciela.

Rozgrywka odbywa się w czasie rzeczywistym.


## 3. Budynki i jednostki

Każdy budynek jest jednego z następujących rodzajów:

* baza, jednego z 4 rodzai: czołgowa, helikopterowa, poduszkowcowa, buforowa
* działko, jednego z 3 rodzai: zwykłe, szybkostrzelne, rakietowe
* wieża lecznicza.

Każdy gracz ma pewną, zmienną w trakcie rozgrywki liczbę jednostek. Jednostki gracza rodzą się w kontrolowanych przez niego bazach ze stałą prędkością (5 jednostek co 10 sekund) aż do wyczerpania pojemności bazy. Wszystkie budynki mają określoną pojemność, równą 50, z wyjątkiem baz, których pojemność wynosi 100.

W przeludnionym budynku jednostki giną ze stałą prędkością 1 jednostki na sekundę.

Budynek, nawet jeśli nie jest neutralny, może być pusty (zawierać zero jednostek).

## 4. Pojazdy

Po mapie poruszają się pojazdy. Pojazd porusza się w sposób ciągły i w każdej chwili ma określoną pozycję — środek swojej grafiki.
Każdy pojazd przynależy do któregoś z graczy i jest jednego z następujących rodzajów:
* czołg,
* helikopter,
* poduszkowiec,
* bufor.

Pojazdy powstają, gdy jakiś gracz postanowi przesłać jednostki z jednego budynku do drugiego i ich typ zależy od budynku, z którego wyjeżdżają. Jeśli wyjeżdżają z bazy, ich rodzaj zależy od rodzaju tej bazy. Z innych budynków źródłowych zawsze wyjeżdża czołg. Na jednym polu może znajdywać się dowolna ilość pojazdów.

Każdym pojazdem przemieszcza się pewna, niezerowa liczba jednostek. Na początku liczba ta jest równa liczbie jednostek w budynku, z którego dany pojazd wyjechał. Pojazd zabiera wszystkie jednostki z budynku (w budynku pozostaje zero jednostek). Liczba jednostek w pojeździe może ulec zmianie: zwiększa się w przypadku leczenia (od bufora lub wieży leczniczej), a zmniejsza w przypadku:

* walki z innym pojazdem,
* natrafienia na pułapkę ogniową (wtedy obrażenia zadawane są ze stałą prędkością 1/sekundę dopóki na niej jest),
* natrafienia na minę (wtedy zadawana jest stała liczba 25 obrażeń, a następnie ta mina jest usuwana),
* lub dostania pociskiem od działka.

Gdy liczba jednostek w pojeździe przestanie być dodatnia, pojazd znika.

Każdy pojazd ma określony cel podróży (wskazany budynek, dowolny do którego istnieje droga i jest różny od źródłowego), do którego podąża po automatycznie wyznaczonej ścieżce (ścieżka jest wyznaczana w momencie rozpoczęcia podróży i nie może się później zmienić).
Większość pojazdów (wszystkich poza helikopterem) porusza się jedynie po polach leżących na tej samej wysokości. Zmiana wysokości jest możliwa tylko za pomocą podjazdów oraz przejechania pomiędzy lądem o wysokości 1 i wodą przez poduszkowiec.

Gdy pojazd z p jednostkami dociera do budynku docelowego, w którym znajduje się b jednostek, to wykonywana jest jedna z poniższych akcji i pojazd znika:

* Jeśli budynek i pojazd mają wspólnego obecnego właściciela, to liczba jednostek w budynku b zwiększa się o p.
* Jeśli budynek nie ma obecnie właściciela albo ma obecnego właściciela innego niż pojazd, to:
  * jeśli p > b, to budynek staje się własnością właściciela pojazdu i liczba jednostek w budynku jest ustawiana na p-b.
  * w przeciwnym razie, budynek nie zmienia właściciela i liczba jednostek w budynku jest ustawiana na b-p.

Każdy pojazd porusza się z określoną dla jego typu prędkością. Szybkość pojazdu (innego niż helikopter) jest redukowana o połowę, gdy przejeżdża on przez pułapkę lodową. Gdy na drodze pojazdu (innego niż helikopter) pojawi się ściana, to zatrzymuje się on i zaczyna ją atakować z częstotliwością 1 atak na sekundę. Po zniszczeniu ściany, czyli po strzeleniu jej 20 razy, pojazd kontynuuje swoją podróż. Pojazd zatrzymuje się też w przypadku wykrycia wrogiego (czyli należącego do innego gracza) pojazdu w pobliżu. Wtedy te dwa pojazdy zaczynają walkę.

## 5. Typy pojazdów

### 5.1. Czołg

Czołg jest pojazdem lądowym. To oznacza, że może poruszać się tylko po lądzie. Oddziałują na niego wszelkie przeszkody umieszczone na lądzie. Porusza się z prędkością 60 j/s.

### 5.2. Helikopter

Helikopter jest pojazdem latającym. Może poruszać się wszędzie: nad wodą i nad lądem. Jest jedynym pojazdem, którego nie dotyczą różnice wysokości pól. Ponadto nie oddziałują na niego żadne utrudnienia. Porusza się z prędkością 90 j/s.

### 5.3. Poduszkowiec

Poduszkowiec to pojazd lądowo-wodny. Może się poruszać na lądzie i w wodzie. Lecz przejazd pomiędzy wodą i lądem jest możliwy tylko gdy ląd jest na wysokości 1. Oddziałują na niego wszelkie przeszkody umieszczone na lądzie i w wodzie. Porusza on się 20% wolniej niż inne pojazdy, czyli z prędkością 48 j/s.

### 5.4. Bufor

Bufor ma taki ruch, jak czołg. Jedyną różnicą między buforem a czołgiem jest to, że bufor leczy wszystkie przyjazne (tj. mające tego samego właściciela) dla niego pojazdy w pobliżu: leczenie dotyczy pojazdów, których aktualna pozycja leży w okręgu o promieniu równym zasięgowi bufora, wyznaczonym wokół aktualnej pozycji bufora (odległości mierzymy jak w sekcji 9). Może on leczyć inne bufory, ale nie leczy samego siebie. Promień leczenia bufora wynosi 160 j.


## 6. Sterowanie

Każdy gracz może wysłać pojazd z jednostkami. W tym celu, gracz najpierw wskazuje swój budynek (z dodatnią liczbą jednostek w środku), z którego chce wysyłać jednostki. Następnie wskazuje dowolny (inny) budynek docelowy. Wtedy wyznaczana jest najkrótsza możliwa droga między wskazanymi budynkami, która zależy od typu pojazdu i nie uwzględnia utrudnień (min, pułapek i ścian). Jeśli nie istnieje żadna droga, pojazd nie jest wysyłany. Pojazd zabiera wszystkie jednostki z budynku źródłowego.



## 7. Podjazdy

Podjazd jest specjalnym przejściem pomiędzy obszarami znajdującymi się na różnych wysokościach.
Podjazd zajmuje całe pole (nazwijmy je p) i łączy ze sobą dwa przeciwległe pola (nazwijmy je a i b) sąsiadujące z p.
Na pole p można wjechać tylko z pól a oraz b, a także z p można zjechać tylko na pole a lub b, i przejazdy w obie strony są możliwe bez względu na wysokość pól a oraz b.
Pole p znajduje się na wysokości równej minimum z wysokości pól a oraz b.

## 8. Mosty

Most łączy ze sobą dwa niesąsiadujące ze sobą pola (nazwijmy je a i b) będące na tej samej wysokości (oznaczmy ją literą w).
Most leży na jednym albo więcej pól o wysokościach mniejszych od w-2 (w szczególności nie obejmuje samych pól a oraz b).
Most zaczyna się na polu sąsiadującym z a, kończy na polu sąsiadującym z b i nigdy nie skręca (musi istnieć odcinek łączący a z b, prostopadły do ścianek pól).
Przejazd jest możliwy albo po moście (wzdłuż mostu), albo pod mostem (zgodnie z regułami opisanymi wcześniej).
Helikopter lata dowolnie, nad mostem.

## 9. Walka pojazdów
Odległość między dwoma pojazdami to odległość euklidesowa między ich aktualnymi pozycjami. Odległość pojazdu od nieruchomego obiektu (budynku, działka, wieży leczniczej) to odległość euklidesowa między aktualną pozycją pojazdu a środkiem pola, na którym ten obiekt stoi.

Odległości, promienie i zasięgi wyrażamy w jednostkach odległości (j). Jednostka j jest umowna — jej przeliczenie na piksele określa specyfikacja implementacji.

Pojazd cyklicznie (co tick symulacji) wykrywa wrogie (należące do innych graczy) pojazdy znajdujące się w odległości nie większej niż 80 j — obszar wykrywania ma kształt koła o promieniu 80 j (promień wykrywania) wokół pozycji pojazdu. Gdy dwa wrogie pojazdy spotkają się na trasie, rozpoczyna się walka. Pojazd rozpoczyna walkę z najbliższym z wykrytych, czyli o najmniejszej odległości — jest on „wykrytym jako pierwszy”. W trakcie trwającej walki pojazd nie zmienia celu: walczy z danym przeciwnikiem, dopóki któryś z nich nie zostanie pokonany, nawet jeśli inny wróg zbliży się na mniejszą odległość.

Jeśli odległości kilku wykrytych pojazdów są identyczne, celem jest ten, który wcześniej wszedł w obszar wykrywania; przy pełnym remisie wybór jest deterministyczny (pojazd znajdujący się na polu o mniejszej współrzędnej q, a przy równości — r, w układzie współrzędnych axialnych).

Walka polega na tym, że każdy pojazd co sekundę wysyła pocisk do wrogiego pojazdu, zmniejszając ilość jego jednostek o dokładnie $\lceil x \div 5 \rceil$, gdzie $x$ oznacza ilość jednostek w pojeździe, który wysłał pocisk.

Gdy do walczących pojazdów dołącza jeszcze jeden, to zaczyna on atakować pojazd wykryty jako pierwszy (najbliższy), chyba że jest on przyjazny — wtedy go ignoruje.  

Walka trwa, dopóki jedna ze stron nie zostanie pokonana (ilość jednostek w pojeździe jednej ze stron nie spadnie do zera).
Po wygraniu walki zwycięski pojazd kontynuuje swoją wcześniej wyznaczoną trasę.


## 10. Działka

Działka to nieruchome struktury bojowe.

Występują trzy rodzaje.

Każde działko:
* posiada swój określony zasięg,
* automatycznie strzela pociski do wrogich pojazdów znajdujących się w jego zasięgu,
* jego pocisk zmniejsza liczbę jednostek w pojeździe, który nim został trafiony
* nie produkuje nowych jednostek.

Pojazd znajduje się w zasięgu działka, jeśli jego aktualna pozycja leży w okręgu o promieniu równym zasięgowi działka, wyznaczonym wokół środka pola działka (odległości mierzymy jak w sekcji 9).

W przypadku wielu wrogów działko strzela do najbliższego. Trafienie pociskiem jest rozliczane w momencie wystrzelenia pocisku, który ma charakter czysto wizualny. 

przez $x$ oznaczmy ilość jednostek w działku:

### 10.1. Zwykłe działko

strzela z prędkością 1 pocisk na 5s, a każdy pocisk zadaje $x$ obrażeń. Zasięg działania: 250 j.

### 10.2. Działko rakietowe

Jego działanie różni się od zwykłego działka tylko tym, że jego pocisk oprócz trafionego pojazdu zmniejsza liczbę jednostek także wszystkim wrogim pojazdom w pobliżu. Ponadto ma większy zasięg niż działka pozostałych typów. Zasięg działania: 375 j, promień obszaru rażenia wokół trafionego pojazdu: 160 j.

### 10.3. Działko szybkostrzelne

strzela z prędkością 1 pocisk na 1s, a każdy pocisk zadaje $\lceil x \div 4 \rceil$ obrażeń. Zasięg działania: 190 j.

## 11. Wieża lecznicza

Wieże lecznicze są nieruchomymi strukturami bojowymi.

Każda wieża lecznicza:
* posiada zasięg równy 80 j + x, gdzie x oznacza ilość jednostek w wieży
* automatycznie zwiększa ilość jednostek o 2 każdemu przyjaznemu pojazdowi znajdującemu się w jej zasięgu co 3 s,
* nie produkuje nowych jednostek.

Pojazd znajduje się w zasięgu wieży leczniczej, jeśli jego aktualna pozycja leży w okręgu o promieniu równym zasięgowi wieży, wyznaczonym wokół środka pola wieży (odległości mierzymy jak w sekcji 9).

## 12. Budynki neutralne

Budynki neutralne nie mogą wysyłać pojazdów, bazy neutralne nie produkują jednostek, neutralne wieże lecznicze nie dodają nikomu jednostek, lecz neutralne działka strzelają do wszystkich pojazdów, wg zasad z sekcji 10, pociskami zadającymi normalną ilość obrażeń.

