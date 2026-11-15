(ns shop.orders
  (:require [shop.pricing :as pricing]))

(defrecord Order [id lines]
  pricing/Priced
  (price [this]
    (let [s (subtotal this)]
      (+ s (pricing/tax s)))))

(defn new-order [id]
  (->Order id []))

(defn add-line [order qty unit-cents]
  (if (pos? qty)
    (update order :lines conj {:qty qty :unit-cents unit-cents})
    order))

(defn subtotal [order]
  (reduce + (map (fn [{:keys [qty unit-cents]}]
                   (if (> qty 10)
                     (quot (* unit-cents qty 9) 10)
                     (* unit-cents qty)))
                 (:lines order))))
