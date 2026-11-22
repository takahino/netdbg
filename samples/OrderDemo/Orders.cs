using System;
using System.Collections.Generic;
using System.Data.SqlClient;
using System.Linq;

namespace OrderDemo
{
    /// <summary>A product on the menu.</summary>
    public class Product
    {
        public string Name { get; }
        public decimal Price { get; }
        public int Stock { get; set; }

        public Product(string name, decimal price, int stock)
        {
            Name = name;
            Price = price;
            Stock = stock;
        }

        public override string ToString() => $"{Name} ({Price:N0})";
    }

    /// <summary>One line of an order.</summary>
    public class OrderLine
    {
        public Product Product { get; }
        public int Quantity { get; }

        public OrderLine(Product product, int quantity)
        {
            Product = product;
            Quantity = quantity;
        }

        public decimal Subtotal => Product.Price * Quantity;

        public override string ToString() => $"{Product.Name} x {Quantity} = {Subtotal:N0}";
    }

    /// <summary>Thrown when an order asks for more than is left.</summary>
    public class OutOfStockException : Exception
    {
        public OutOfStockException(Product product, int wanted)
            : base($"Only {product.Stock} {product.Name} left (wanted {wanted}).")
        {
        }
    }

    /// <summary>An order: lines, and the total with the bulk discount.</summary>
    public class Order
    {
        /// <summary>Lines of at least this many items get the bulk discount.</summary>
        public const int BulkQuantity = 3;

        public string Customer { get; set; } = "";
        public List<OrderLine> Lines { get; } = new List<OrderLine>();
        public decimal DiscountRate { get; set; } = 0.10m;

        public void Add(Product product, int quantity)
        {
            if (quantity > product.Stock)
            {
                throw new OutOfStockException(product, quantity);
            }
            product.Stock -= quantity;
            Lines.Add(new OrderLine(product, quantity));
        }

        public decimal Total()
        {
            decimal total = 0;
            foreach (var line in Lines)
            {
                decimal subtotal = line.Subtotal;
                if (line.Quantity >= BulkQuantity)
                {
                    subtotal -= subtotal * DiscountRate;
                }
                total += subtotal;
            }
            return total;
        }

        /// <summary>The lines that get the bulk discount.</summary>
        public IEnumerable<OrderLine> DiscountedLines() => Lines.Where(line => line.Quantity >= BulkQuantity);
    }

    /// <summary>
    /// Saves orders with ADO.NET. The sample has no database: the commands are made and run, so a debugger can record
    /// their SQL and parameters, and then they fail because there is no connection.
    /// </summary>
    public static class OrderRepository
    {
        /// <summary>Returns how many commands ran (none, in the sample).</summary>
        public static int Save(Order order)
        {
            var header = new SqlCommand("INSERT INTO Orders (Customer, Total, Placed) VALUES (@customer, @total, @placed)");
            header.Parameters.AddWithValue("@customer", order.Customer);
            header.Parameters.AddWithValue("@total", order.Total());
            header.Parameters.AddWithValue("@placed", DateTime.Now);
            int saved = Run(header);
            foreach (var line in order.Lines)
            {
                var item = new SqlCommand("INSERT INTO OrderLines (Product, Quantity, Price) VALUES (@product, @quantity, @price)");
                item.Parameters.AddWithValue("@product", line.Product.Name);
                item.Parameters.AddWithValue("@quantity", line.Quantity);
                item.Parameters.AddWithValue("@price", line.Product.Price);
                saved += Run(item);
            }
            return saved;
        }

        private static int Run(SqlCommand command)
        {
            try
            {
                command.ExecuteNonQuery();
                return 1;
            }
            catch (InvalidOperationException)
            {
                // No database in the sample: the command has no connection.
                return 0;
            }
        }
    }
}
