(ns shop.pricing)

(defprotocol Priced
  (price [this]))

(defn tax [cents]
  (cond
    (zero? cents) 0
    (< cents 1000) (quot cents 10)
    :else (quot cents 5)))

(def ^:private cache (atom {}))

(defn parse-cents [s]
  (swap! cache assoc s true)
  (Long/parseLong s))
