# Introduction
L’allocateur SLUB a été conçu par Christoph Lameter en tant qu’évolution de l’allocateur SLAB, visant à améliorer les performances, la scalabilité et la consommation mémoire.

Le principe de l’allocateur SLAB est d’avoir pour chaque type d’objets un cache contenant des listes appelées slab qui stockent ceux-ci dans un état initialisé. Ce qui permet une allocation plus rapide et efficace. Cependant, il souffre d’une surcharge de métadonnées et d’une faible scalabilité sur les systèmes multiprocesseurs, ce qui peut entraîner une surconsommation mémoire.

# Fonctionnement
Dans l’allocateur SLUB, les slab ne sont plus gérés via des structures séparées, ce sont directement les pages qui contient les informations. Pour chaque objet d’une taille donnée. Il n’y a plus de métadonnées globales par slab. Les objets libres sont chaînés entre eux via une liste stockée dans les objets eux-mêmes.

Pour retrouver cette liste, l’allocateur ajout trois nouveau champs à la structure des pages associées à un slab. 

```
void *freelist
short unsigned int inuse
short unsigned int offset
```

`freelist` est un pointeur vers le premier objet libre du slab
`inuse` indique le nombre d’objet alloués dans un slab
`offset` indique à l’allocateur ou trouver `freelist`

Quand un slab est créé et reçoit son premier objet, il obtient le statut _partial_ : cela indique qu’il contient au moins un objet, mais qu’il reste encore de la place pour en recevoir d’autres.

Afin d’améliorer la scalabilité, l’allocateur SLUB crée une liste _partial_ pour chaque nœud NUMA (Non-Uniform Memory Access). Un nœud NUMA est un ensemble de processeurs et de mémoire physiquement proches, pour lesquels l’accès à la mémoire locale est plus rapide que l’accès à la mémoire distante.

Il existe également un slab actif par CPU et par cache d’objets, ce qui permet de limiter les accès concurrents et d’améliorer les performances. Lorsqu’un slab actif n’est plus utilisé, il peut être renvoyé vers une liste _partial_ appropriée.

Lorsque tous les objets d’un slab sont alloués, celui-ci est retiré des listes _partial_ et marqué comme _full_. Il ne sera de nouveau pris en compte que lorsqu’un objet sera libéré, auquel cas il repassera dans une liste _partial_ adaptée.

Dans le cas où tous les objets d’un slab sont libérés (`inuse == 0`), le slab est renvoyé vers l’allocateur de pages afin d’être recyclé.

Un autre aspect intéressant de l’allocateur SLUB est sa capacité à combiner des slabs d’objet de taille et de paramètres similaires. Cela permet de réduire le nombre total de caches et de limiter la fragmentation mémoire.
