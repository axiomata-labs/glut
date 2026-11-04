`split` in `money.py` loses or adds cents: `split(10.00, 3)` gives parts that do not add up to 10.00. Fix it: the parts
add up to the total to the cent, differ by one cent at most, and the larger ones come first.
