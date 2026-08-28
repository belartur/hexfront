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

Plansza gry może być dość duża i nie mieścić się na ekranie. Wtedy gracz może przesuwać widok, aby obserwować różne części pola bitwy.


## 2. Cel Gry

Celem gry jest przejęcie kontroli nad wszystkimi budynkami na mapie.
Gracz kontrolowany przez człowieka zwycięża po przejęciu wszystkich budynków.
Jeśli zaś straci on wszystkie budynki, to gra się kończy przegraną.

Po przejęciu budynku, zaczyna on działać na rzecz nowego właściciela.

Rozgrywka odbywa się w czasie rzeczywistym.


## 3. Budynki i jednostki

Każdy budynek jest jednego z następujących rodzajów:

* baza, jednego z 4 rodzai: czołgowa, helikopterowa, poduszkowcowa, buforowa
* działko, jednego z 3 rodzai: zwykłe, szybkostrzelne, rakietowe
* wieża lecznicza.

Każdy gracz ma pewną, zmienną w trakcie rozgrywki liczbę jednostek. Jednostki gracza rodzą się w kontrolowanych przez niego bazach ze stałą prędkością (5 jednostek co 10 sekund) aż do wyczerpania pojemności bazy. Wszystkie budynki mają określoną pojemność, równą 50, z wyjątkiem baz, których pojemność wynosi 100.

W przeludnionym budynku jednostki giną ze stała prędkością 1 jednostki na sekundę.

Budynek, nawet jeśli nie jest neutralny, może być pusty (zawierać zero jednostek).


## 4. Pojazdy

Po mapie poruszają się pojazdy. Każdy pojazd przynależy do któregoś z graczy i jest jednego z następujących rodzajów:
* czołg,
* helikopter,
* poduszkowiec,
* bufor.

Pojazdy powstają, gdy jakiś gracz postanowi przesłać jednostki z jednego budynku do drugiego i ich typ zależy od budynku, z którego wyjeżdżają. Jeśli wyjeżdżają z bazy, ich rodzaj zależy od rodzaju tej bazy. Z innych budynków źródłowych zawsze wyjeżdża czołg.

Każdym pojazdem przemieszcza się pewna, niezerowa liczba jednostek. Na początku liczba ta jest równa liczbie jednostek w budynku, z którego dany pojazd wyjechał. Pojazd zabiera wszystkie jednostki z budynku (w budynku pozostaje zero jednostek). Liczba jednostek w pojeździe może ulec zmianie: zwiększa się w przypadku leczenia (od bufora lub wieży leczniczej), a zmniejsza w przypadku:

* walki z innym pojazdem,
* natrafienia na pułapkę ogniową (wtedy obrażenia zadawane są z stałą prędkością 1/sekundę dopóki na niej jest),
* natrafienia na minę (wtedy zadawana jest stała liczba 25 obrażeń, a następnie ta mina jest usuwana),
* lub dostania pociskiem od działka.

Gdy liczba jednostek w pojeździe przestanie być dodatnia, pojazd znika.

Każdy pojazd ma określony cel podróży (wskazany budynek, dowolny do którego istnieje droga i jest różny od źródłowego), do którego podąża po automatycznie wyznaczonej ścieżce (ścieżka jest wyznaczana w momencie rozpoczęcia podróży i nie może się później zmienić).
Większości pojazdów (wszystkich poza helikopterami) zazwyczaj może poruszać się jedynie po polach leżących na tej samej wysokości. Zmiana wysokości jest możliwa tylko za pomocą podjazdów oraz przejechania pomiędzy lądem o wysokości 1 i wodą przez poduszkowiec.

Gdy pojazd z p jednostkami dociera do budynku docelowego, w którym znajduje się b jednostek, to wykonywana jest jedna z poniższych akcji i pojazd znika:

* Jeśli budynek i pojazd mają wspólnego właściciela, to liczba jednostek w budynku b zwiększa się o p.
* Jeśli budynek nie ma właściciela albo ma właściciela innego niż pojazd, to:
  * jeśli p > b, to budynek staje się własnością właściciela pojazdu i liczba jednostek w budynku jest ustawiana na p-b.
  * w przeciwnym razie, budynek nie zmieniania właściciela i liczba jednostek w budynku jest ustawiana na b-p.

Każdy pojazd porusza się z określoną dla jego typu prędkością. Szybkość pojazdu (innego niż helikopter) jest redukowana o połowę, gdy przejeżdża on przez pułapkę lodową. Gdy na drodze pojazdu (innego niż helikopter) pojawi się ściana, to zatrzymuje się on i zaczyna ją atakować. Pojazd kontynuuje swoją podróż po zniszczeniu ściany, czyli po strzeleniu jej 20 razy. Pojazd zatrzymuje się też w przypadku wykrycia wrogiego (czyli należącego do innego gracza) pojazdu w pobliżu. Wtedy te dwa pojazdy zaczynają walkę.

## 5. Typy pojazdów

### 5.1. Czołg

Czołg jest pojazdem lądowym. To oznacza, że może poruszać się tylko po lądzie. Oddziałują na niego wszelkie przeszkody umieszczone na lądzie.

### 5.2. Helikopter

Helikopter jest pojazdem latającym. Może poruszać się wszędzie: nad wodą i nad lądem. Jest jedynym pojazdem, które nie dotyczą różnice wysokości pól. Ponadto nie oddziałują na niego żadne utrudnienia.

### 5.3. Poduszkowiec

Poduszkowiec to pojazd lądowo-wodny. Może się poruszać na lądzie i w wodzie. Lecz przejazd pomiędzy wodą i lądem jest możliwy tylko gdy ląd jest na wysokości 1. Oddziałują na niego Wszelkie przeszkody umieszczone na lądzie i w wodzie. Porusza on się wolniej niż inne pojazdy.

### 5.4. Bufor

Bufor ma taki ruch, jak czołg. To, co jedyną różnicą między buforem i czołgiem, jest fakt, że bufor leczy wszystkie przyjazne (tj. mające tego samego właściciela) dla niego pojazdy w pobliżu.


## 6. Sterowanie

Każdy gracz może wysłać pojazd z jednostkami. W tym celu, gracz najpierw wskazuje swój budynek (z dodatnią liczbą jednostek w środku), z którego chce wysyłać jednostki (ten budynek zostaje zaznaczony w UI). Następnie wskazuje dowolny (inny) budynek docelowy. Wtedy wyznaczana jest najkrótsza możliwa droga między wskazanymi budynkami, która jednak nie uwzględnia utrudnień(min, pułapek i ścian) (i budynek źródłowy jest odznaczany w UI). Jeśli nie istnieje żadna droga, pojazd nie jest wysyłany. Pojazd zabiera wszystkie jednostki z budynku źródłowego.



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

Gdy dwa wrogie (należące do innych graczy) pojazdy spotkają się na trasie, rozpoczyna się walka.
Walka polega na tym, że każdy pojazd co sekundę wysyła pocisk do wrogiego pojazdu, zmniejszając ilość jego jednostek o dokładnie $\lceil x \div 5 \rceil$, gdzie $x$ oznacza ilość jednostek w pojeździe, który wysłał pocisk.
Walka trwa, dopóki jedna ze stron nie zostanie pokonana (ilość jednostek w pojeździe jednej ze stron nie spadnie do zera).
Po wygraniu walki zwycięski pojazd kontynuuje swoją wcześniej wyznaczoną trasę.

## 10. Działka

Działka są nieruchomymi strukturami bojowymi.

Występują trzy rodzaje.

Każde działko:
* posiada swój określony zasięg,
* automatycznie strzela pociski do wrogich pojazdów znajdujących się w jego zasięgu,
* Jego pocisk zmniejsza ilość jednostek w pojeździe, który nim dostał
* nie produkuje nowych jednostek.

przez $x$ oznaczmy ilość jednostek w działku:

### 10.1. Zwykłe działko

strzela z prędkością 1 pocisk na 5s, a każdy pocisk zadaje $x$ obrażeń.

### 10.2. Działko rakietowe

Jej działanie różni się od zwykłego działka tylko tym, że jej pocisk oprócz pojazdowi trafionemu zmniejsza ilość jednostek wszystkim wrogim pojazdom w pobliżu. Ponadto ma większy zasięg niż działka pozostałych typów.

### 10.3. Działko szybkostrzelne

strzela z prędkością 1 pocisk na 1s, a każdy pocisk zadaje $\lceil x \div 4 \rceil$ obrażeń.

## 11. Wieża lecznicza

Wieże lecznicze są nieruchomymi strukturami bojowymi.

Każda wieża lecznicza:
* posiada swój określony zasięg, zależny od ilości jednostek
* automatycznie zwiększa ilość jednostek o 2 przyjaznym pojazdom znajdującym się w jego zasięgu co 3 s,
* nie produkuje nowych jednostek.
